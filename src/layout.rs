use crate::config::{Config, HomeMode};
use crate::error::ReloError;
use crate::version::{parse_release, resolve, Release};
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use std::borrow::Cow;
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

    pub fn default_release(&self) -> Result<Release> {
        match self.active_version()? {
            Some(id) => self.resolve(&id),
            None => self.resolve("latest"),
        }
    }

    pub fn set_active(&self, id: &str) -> Result<()> {
        let active = self.active_path();
        if active.exists() || fs::symlink_metadata(&active).is_ok() {
            let meta = fs::symlink_metadata(&active)?;
            if !meta.file_type().is_symlink() {
                return Err(ReloError::ActiveNotSymlink(active.display().to_string()).into());
            }
            remove_active_symlink(&active)?;
        }
        // Store a relative link so a managed context can be moved as a directory.
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

    pub fn effective_env(&self, release_id: &str) -> Result<IndexMap<String, String>> {
        let mut env = IndexMap::new();
        for (name, value) in &self.config.env {
            let value = self.expand_variables(value, release_id, &env)?;
            env.insert(name.clone(), value);
        }
        if let Some(release) = self.release_config(release_id) {
            for (name, value) in &release.env {
                let value = self.expand_variables(value, release_id, &env)?;
                env.shift_remove(name);
                env.insert(name.clone(), value);
            }
        }
        Ok(env)
    }

    pub fn effective_path(&self, release_id: &str, use_path: &[String]) -> Result<Vec<PathBuf>> {
        let env = self.effective_env(release_id)?;
        use_path
            .iter()
            .chain(
                self.release_config(release_id)
                    .into_iter()
                    .flat_map(|release| release.path.iter()),
            )
            .chain(self.config.path.iter())
            .map(|path| {
                self.expand_path_value(path, release_id, &env)
                    .map(|value| self.resolve_config_path(&value))
            })
            .collect()
    }

    fn release_config(&self, release_id: &str) -> Option<&crate::config::ReleaseConfig> {
        self.config
            .releases
            .iter()
            .find(|release| release.id == release_id)
    }

    fn resolve_config_path(&self, value: &str) -> PathBuf {
        let path = Path::new(value);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        self.root.join(path)
    }

    fn validate_active(&self) -> Result<()> {
        self.active_version().map(|_| ())
    }

    fn expand_variables(
        &self,
        value: &str,
        release_id: &str,
        env: &IndexMap<String, String>,
    ) -> Result<String> {
        let expanded = shellexpand::env_with_context(value, |name| {
            self.lookup_config_value(name, release_id, env)
        })
        .map_err(|err| anyhow::anyhow!("failed to expand {value:?}: {err}"))?
        .into_owned();
        Ok(shellexpand::tilde(&expanded).into_owned())
    }

    fn expand_path_value(
        &self,
        value: &str,
        release_id: &str,
        env: &IndexMap<String, String>,
    ) -> Result<String> {
        let expanded = self.expand_variables(value, release_id, env)?;
        Ok(expanded)
    }

    fn lookup_config_value(
        &self,
        name: &str,
        release_id: &str,
        env: &IndexMap<String, String>,
    ) -> Result<Option<Cow<'static, str>>> {
        if let Some(name) = name.strip_prefix("relo.") {
            let value = match name {
                "context" | "ctx" | "root" => self.root.display().to_string(),
                "active" => self.active_path().display().to_string(),
                "release" => self.release_path(release_id).display().to_string(),
                "home" => self.home_for(release_id).display().to_string(),
                "version" => release_id.to_string(),
                _ => bail!("unknown variable: relo.{name}"),
            };
            return Ok(Some(Cow::Owned(value)));
        }

        if let Some(name) = name.strip_prefix("env.") {
            let Some(value) = env.get(name) else {
                bail!("unknown variable: env.{name}");
            };
            return Ok(Some(Cow::Owned(value.clone())));
        }

        if let Some(name) = name.strip_prefix("sys.") {
            let value = std::env::var(name)
                .with_context(|| format!("unknown system environment variable: sys.{name}"))?;
            return Ok(Some(Cow::Owned(value)));
        }

        bail!("unknown variable: {name}");
    }
}

#[cfg(unix)]
fn create_symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<()> {
    std::os::unix::fs::symlink(src, dst)?;
    Ok(())
}

#[cfg(unix)]
fn remove_active_symlink(path: &Path) -> Result<()> {
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)?;
    Ok(())
}

#[cfg(windows)]
fn remove_active_symlink(path: &Path) -> Result<()> {
    fs::remove_dir(path)?;
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
