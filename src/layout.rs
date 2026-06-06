use crate::config::{Config, EnvValue, HomeMode};
use crate::error::ReloError;
use crate::version::{parse_release, resolve, Release};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct Layout {
    pub root: PathBuf,
    pub config: Config,
}

impl Layout {
    pub fn init(root: &Path, mode: HomeMode, path: Vec<String>, force: bool) -> Result<()> {
        let config_path = root.join("relo.yaml");
        if config_path.exists() && !force {
            anyhow::bail!(
                "{} already exists; use --force to overwrite it",
                config_path.display()
            );
        }
        fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
        fs::create_dir_all(root.join("releases"))?;
        match mode {
            HomeMode::Shared => fs::create_dir_all(root.join("home"))?,
            HomeMode::Versioned => fs::create_dir_all(root.join("homes"))?,
        }
        let config = Config::default_for(root, mode, path);
        config.write(&config_path)?;
        Ok(())
    }

    pub fn load(root: PathBuf) -> Result<Self> {
        let config_path = root.join("relo.yaml");
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
        self.root.join("relo.yaml")
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

    pub fn effective_env(&self, release_id: &str) -> BTreeMap<String, String> {
        let mut env = self.config.env.clone();
        if let Some(release) = self.release_config(release_id) {
            env.extend(release.env.clone());
        }
        env.into_iter()
            .map(|(name, value)| {
                let value = match value {
                    EnvValue::Path { path } => self
                        .resolve_config_path(&path, release_id)
                        .display()
                        .to_string(),
                    EnvValue::Value { value } => value,
                };
                (name, value)
            })
            .collect()
    }

    pub fn effective_path(&self, release_id: &str, use_path: &[String]) -> Vec<PathBuf> {
        use_path
            .iter()
            .chain(
                self.release_config(release_id)
                    .into_iter()
                    .flat_map(|release| release.path.iter()),
            )
            .chain(self.config.path.iter())
            .map(|path| self.resolve_config_path(path, release_id))
            .collect()
    }

    fn release_config(&self, release_id: &str) -> Option<&crate::config::ReleaseConfig> {
        self.config
            .releases
            .iter()
            .find(|release| release.id == release_id)
    }

    fn resolve_config_path(&self, value: &str, release_id: &str) -> PathBuf {
        let path = Path::new(value);
        if path.is_absolute() {
            return path.to_path_buf();
        }

        let mut components = path.components();
        let Some(Component::Normal(first)) = components.next() else {
            return self.root.join(path);
        };
        let rest = components.as_path();
        match first.to_str() {
            Some("root") => join_optional_rest(self.root.clone(), rest),
            Some("active") | Some("release") => {
                join_optional_rest(self.release_path(release_id), rest)
            }
            Some("home") => join_optional_rest(self.home_for(release_id), rest),
            _ => self.root.join(path),
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

fn join_optional_rest(base: PathBuf, rest: &Path) -> PathBuf {
    if rest.as_os_str().is_empty() {
        base
    } else {
        base.join(rest)
    }
}
