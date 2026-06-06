use crate::cli::HomeArg;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HomeMode {
    Shared,
    Versioned,
}

impl HomeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            HomeMode::Shared => "shared",
            HomeMode::Versioned => "versioned",
        }
    }
}

impl From<HomeArg> for HomeMode {
    fn from(value: HomeArg) -> Self {
        match value {
            HomeArg::Shared => HomeMode::Shared,
            HomeArg::Versioned => HomeMode::Versioned,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub name: String,
    pub home_mode: HomeMode,
    pub version_separator: String,
    pub path: PathConfig,
    #[serde(default)]
    pub env: BTreeMap<String, EnvValue>,
    #[serde(default)]
    pub releases: Vec<ReleaseConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PathConfig {
    #[serde(default)]
    pub prepend: Vec<String>,
    #[serde(default)]
    pub append: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReleaseConfig {
    pub id: String,
    #[serde(default)]
    pub path: PathConfig,
    #[serde(default)]
    pub env: BTreeMap<String, EnvValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EnvValue {
    Path { path: String },
    Value { value: String },
}

impl Config {
    pub fn default_for(
        root: &Path,
        home_mode: HomeMode,
        path_prepend: Vec<String>,
        path_append: Vec<String>,
    ) -> Self {
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("relo")
            .to_string();
        let prepend = if path_prepend.is_empty() {
            vec!["active".to_string()]
        } else {
            path_prepend
        };
        Self {
            name,
            home_mode,
            version_separator: "_".to_string(),
            path: PathConfig {
                prepend,
                append: path_append,
            },
            env: BTreeMap::new(),
            releases: Vec::new(),
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        let config: Self = serde_yml::from_str(&text)
            .with_context(|| format!("invalid relo.yaml: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let text = serde_yml::to_string(self)?;
        std::fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
    }

    fn validate(&self) -> Result<()> {
        if self.home_mode != HomeMode::Shared && self.home_mode != HomeMode::Versioned {
            bail!("invalid home_mode");
        }
        if self.version_separator != "_" {
            bail!("unsupported version_separator: {}", self.version_separator);
        }
        validate_path_config(&self.path)?;
        validate_env(&self.env)?;
        for release in &self.releases {
            if release.id.is_empty() {
                bail!("release id must not be empty");
            }
            validate_path_config(&release.path)?;
            validate_env(&release.env)?;
        }
        Ok(())
    }
}

fn validate_path_config(path: &PathConfig) -> Result<()> {
    for value in path.prepend.iter().chain(path.append.iter()) {
        validate_path_value(value)?;
    }
    Ok(())
}

fn validate_env(env: &BTreeMap<String, EnvValue>) -> Result<()> {
    for (name, value) in env {
        if !is_valid_env_name(name) {
            bail!("invalid env variable name: {name}");
        }
        if let EnvValue::Path { path } = value {
            validate_path_value(path)?;
        }
    }
    Ok(())
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn validate_path_value(value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("path must not be empty");
    }
    for component in Path::new(value).components() {
        match component {
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir | Component::ParentDir => {
                bail!("relative path must not escape root: {value}")
            }
        }
    }
    Ok(())
}
