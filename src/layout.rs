use crate::config::{Config, HomeMode};
use crate::error::ReloError;
use crate::version::{parse_release, resolve, Release};
use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct Layout {
    pub root: PathBuf,
    pub config: Config,
}

impl Layout {
    pub fn init(root: &Path, mode: HomeMode) -> Result<()> {
        fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
        fs::create_dir_all(root.join("releases"))?;
        match mode {
            HomeMode::Shared => fs::create_dir_all(root.join("home"))?,
            HomeMode::Versioned => fs::create_dir_all(root.join("homes"))?,
        }
        let config = Config::default_for(root, mode);
        config.write(&root.join("relo.toml"))?;
        Ok(())
    }

    pub fn load(root: PathBuf) -> Result<Self> {
        let config_path = root.join("relo.toml");
        if !config_path.is_file() {
            return Err(ReloError::NotRoot(root.display().to_string()).into());
        }
        let releases = root.join("releases");
        if !releases.is_dir() {
            return Err(ReloError::MissingReleases(releases.display().to_string()).into());
        }
        let config = Config::read(&config_path)?;
        let layout = Self { root, config };
        // Validate active eagerly so commands fail before printing partial data.
        layout.validate_active()?;
        Ok(layout)
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("relo.toml")
    }

    pub fn active_path(&self) -> PathBuf {
        self.root.join("active")
    }

    pub fn release_path(&self, id: &str) -> PathBuf {
        self.root.join("releases").join(id)
    }

    pub fn releases(&self) -> Result<Vec<Release>> {
        let mut releases = Vec::new();
        for entry in fs::read_dir(self.root.join("releases"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            releases.push(parse_release(&id, entry.path())?);
        }
        // Keep all callers on the same semver ordering contract.
        releases.sort();
        Ok(releases)
    }

    pub fn releases_with_invalid(&self) -> Result<(Vec<Release>, Vec<String>)> {
        let mut releases = Vec::new();
        let mut invalid = Vec::new();
        for entry in fs::read_dir(self.root.join("releases"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            match parse_release(&id, entry.path()) {
                Ok(release) => releases.push(release),
                Err(_) => invalid.push(id),
            }
        }
        releases.sort();
        invalid.sort();
        Ok((releases, invalid))
    }

    pub fn resolve(&self, expr: &str) -> Result<Release> {
        resolve(&self.releases()?, expr)
    }

    pub fn active_version(&self) -> Result<Option<String>> {
        let active = self.active_path();
        // Use symlink_metadata so we inspect the active link itself rather
        // than following it to the release directory.
        let meta = match fs::symlink_metadata(&active) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        if !meta.file_type().is_symlink() {
            return Err(ReloError::ActiveNotSymlink(active.display().to_string()).into());
        }
        let target = fs::read_link(&active)?;
        let id = active_release_id(&target)?;
        if !self.release_path(&id).is_dir() {
            return Err(ReloError::ActiveMissing(id).into());
        }
        parse_release(&id, self.release_path(&id))?;
        Ok(Some(id))
    }

    pub fn active_release(&self) -> Result<Release> {
        let id = self.active_version()?.ok_or(ReloError::NoActive)?;
        self.resolve(&id)
    }

    pub fn set_active(&self, id: &str) -> Result<()> {
        let active = self.active_path();
        if active.exists() || fs::symlink_metadata(&active).is_ok() {
            let meta = fs::symlink_metadata(&active)?;
            if !meta.file_type().is_symlink() {
                return Err(ReloError::ActiveNotSymlink(active.display().to_string()).into());
            }
            fs::remove_file(&active)?;
        }
        // Store a relative link so a managed root can be moved as a directory.
        create_symlink(Path::new("releases").join(id), active)?;
        Ok(())
    }

    pub fn home_for(&self, id: &str) -> PathBuf {
        match self.config.home_mode {
            HomeMode::Shared => self.root.join("home"),
            HomeMode::Versioned => self.root.join("homes").join(id),
        }
    }

    pub fn ensure_home(&self, id: &str) -> Result<()> {
        fs::create_dir_all(self.home_for(id))?;
        Ok(())
    }

    pub fn env_path(&self, key: &str, release_id: &str) -> PathBuf {
        // Config env values are symbolic locations first; unknown values are
        // treated as root-relative paths such as "home/config".
        match key {
            "root" => self.root.clone(),
            "active" | "release" => self.release_path(release_id),
            "home" => self.home_for(release_id),
            rel => self.root.join(rel),
        }
    }

    fn validate_active(&self) -> Result<()> {
        self.active_version().map(|_| ())
    }
}

#[cfg(unix)]
fn create_symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<()> {
    std::os::unix::fs::symlink(src, dst)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)?;
    Ok(())
}

fn active_release_id(target: &Path) -> Result<String> {
    let mut components = target.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Normal(first)), Some(Component::Normal(second)), None)
            if first == OsStr::new("releases") =>
        {
            second
                .to_str()
                .map(|value| value.to_string())
                .ok_or_else(|| ReloError::ActiveInvalidTarget(target.display().to_string()).into())
        }
        _ => Err(ReloError::ActiveInvalidTarget(target.display().to_string()).into()),
    }
}
