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
        Self::load_with_active_validation(root, true)
    }

    pub fn load_for_activation(root: PathBuf) -> Result<Self> {
        Self::load_with_active_validation(root, false)
    }

    fn load_with_active_validation(root: PathBuf, validate_active: bool) -> Result<Self> {
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
        if validate_active {
            // Validate active eagerly so commands fail before printing partial data.
            layout.validate_active()?;
        }
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
        // Keep all callers on the same dotted-version ordering contract.
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
        let target = active_link_target(&active, &meta)?;
        let id = active_release_id(&target, &self.root)?;
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
        let existing_target = match fs::symlink_metadata(&active) {
            Ok(meta) => Some(active_link_target(&active, &meta)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };
        replace_active_link(&self.root, id, &active, existing_target.as_deref())?;
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
            return normalize_path(path);
        }
        normalize_path(&self.root.join(path))
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
        Ok(normalize_expanded_env_value(
            shellexpand::tilde(&expanded).as_ref(),
        ))
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

fn normalize_path(path: &Path) -> PathBuf {
    path.components().collect()
}

fn normalize_expanded_env_value(value: &str) -> String {
    #[cfg(windows)]
    {
        let path = Path::new(value);
        // Config env values are still strings, but path-like built-in values
        // may be extended with `/suffix`. Normalize only a single absolute
        // path, not PATH-style lists or arbitrary literals.
        if path.is_absolute() && !value.contains(';') {
            return normalize_path(path).display().to_string();
        }
    }
    value.to_string()
}

#[cfg(unix)]
fn active_link_target(path: &Path, meta: &fs::Metadata) -> Result<PathBuf> {
    if !meta.file_type().is_symlink() {
        return Err(ReloError::ActiveNotManagedLink(path.display().to_string()).into());
    }
    Ok(fs::read_link(path)?)
}

#[cfg(windows)]
fn active_link_target(path: &Path, meta: &fs::Metadata) -> Result<PathBuf> {
    if meta.file_type().is_symlink() {
        return Ok(fs::read_link(path)?);
    }
    junction::get_target(path)
        .map_err(|_| ReloError::ActiveNotManagedLink(path.display().to_string()).into())
}

#[cfg(unix)]
fn replace_active_link(
    _root: &Path,
    id: &str,
    active: &Path,
    existing_target: Option<&Path>,
) -> Result<()> {
    if existing_target.is_some() {
        fs::remove_file(active)?;
    }
    // Unix symlinks stay relative so the context remains movable.
    std::os::unix::fs::symlink(Path::new("releases").join(id), active)?;
    Ok(())
}

#[cfg(windows)]
fn replace_active_link(
    root: &Path,
    id: &str,
    active: &Path,
    existing_target: Option<&Path>,
) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staged = root.join(format!(".relo-active-{}-{nonce}", std::process::id()));
    let target = root.join("releases").join(id);

    // Creating the replacement first keeps the old active link intact if the
    // filesystem cannot create junctions (for example, on a non-NTFS volume).
    junction::create(&target, &staged)?;
    if existing_target.is_some() {
        if let Err(err) = fs::remove_dir(active) {
            let _ = fs::remove_dir(&staged);
            return Err(err.into());
        }
    }
    if let Err(err) = fs::rename(&staged, active) {
        if let Some(old_target) = existing_target {
            let old_target = if old_target.is_absolute() {
                old_target.to_path_buf()
            } else {
                root.join(old_target)
            };
            let _ = junction::create(old_target, active);
        }
        let _ = fs::remove_dir(&staged);
        return Err(err.into());
    }
    Ok(())
}

fn active_release_id(target: &Path, root: &Path) -> Result<String> {
    let relative = if target.is_absolute() {
        absolute_active_target(target, root)?
    } else {
        Cow::Borrowed(target)
    };
    let mut components = relative.components();
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

fn absolute_active_target<'a>(target: &'a Path, root: &Path) -> Result<Cow<'a, Path>> {
    #[cfg(windows)]
    let root = strip_verbatim_prefix(root);
    #[cfg(not(windows))]
    let root = Cow::Borrowed(root);

    #[cfg(windows)]
    let target = strip_verbatim_prefix(target);
    #[cfg(not(windows))]
    let target = Cow::Borrowed(target);

    target
        .strip_prefix(root.as_ref())
        .map(|path| Cow::Owned(path.to_path_buf()))
        .map_err(|_| ReloError::ActiveInvalidTarget(target.display().to_string()).into())
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: &Path) -> Cow<'_, Path> {
    let value = path.to_string_lossy();
    match value.strip_prefix(r"\\?\") {
        Some(value) => Cow::Owned(PathBuf::from(value)),
        None => Cow::Borrowed(path),
    }
}
