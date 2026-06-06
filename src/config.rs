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
    pub bin: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Config {
    pub fn default_for(root: &Path, home_mode: HomeMode) -> Self {
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("relo")
            .to_string();
        Self {
            name,
            home_mode,
            version_separator: "_".to_string(),
            // active/bin is the common case for release layouts; shell export
            // resolves it to the selected release during local use.
            bin: vec!["active/bin".to_string()],
            env: BTreeMap::new(),
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("invalid relo.toml: {}", path.display()))?;
        if config.home_mode != HomeMode::Shared && config.home_mode != HomeMode::Versioned {
            bail!("invalid home_mode");
        }
        if config.version_separator != "_" {
            bail!(
                "unsupported version_separator: {}",
                config.version_separator
            );
        }
        for name in config.env.keys() {
            if !is_valid_env_name(name) {
                bail!("invalid env variable name: {name}");
            }
        }
        for path in &config.bin {
            validate_root_relative_path(path)?;
        }
        for value in config.env.values() {
            if !matches!(value.as_str(), "root" | "active" | "release" | "home") {
                validate_root_relative_path(value)?;
            }
        }
        Ok(config)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let mut text = toml::to_string_pretty(self)?;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        std::fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
    }
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn validate_root_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        bail!("path must be relative to root: {value}");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("path must be relative to root: {value}"),
        }
    }
    Ok(())
}
