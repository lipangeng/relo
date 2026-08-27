#[cfg(any(test, windows, feature = "check-windows"))]
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CONTEXT_PREFIX: &str = "RELO_CONTEXT_";
pub(super) const RELEASE_PREFIX: &str = "RELO_RELEASE_";
pub(super) const PATH_PREFIX: &str = "RELO_PATH_";
pub(super) const ENV_PREFIX: &str = "RELO_ENV_";
pub(super) const OWNER_PREFIX: &str = "RELO_OWNER_";
pub(super) const PATH_PREPEND: &str = "RELO_PATH_PREPEND";
pub(super) const PATH_APPEND: &str = "RELO_PATH_APPEND";
pub(super) const CONTEXT_ID_LEN: usize = 26;
pub(super) const MAX_ENV_VALUE_LEN: usize = 32_767;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    User,
    System,
}

impl Scope {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ValueKind {
    String,
    ExpandString,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct EnvValue {
    pub(super) name: String,
    pub(super) value: String,
    pub(super) kind: ValueKind,
}

impl EnvValue {
    pub(super) fn string(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            kind: ValueKind::String,
        }
    }

    pub(super) fn expandable(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            kind: ValueKind::ExpandString,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) values: BTreeMap<String, EnvValue>,
}

impl Snapshot {
    #[cfg(any(test, windows, feature = "check-windows"))]
    #[allow(dead_code)]
    pub(super) fn from_values(values: impl IntoIterator<Item = EnvValue>) -> Result<Self> {
        let mut snapshot = Self::default();
        for value in values {
            let key = value.name.to_ascii_uppercase();
            if snapshot.values.insert(key.clone(), value).is_some() {
                bail!("duplicate Windows environment variable: {key}");
            }
        }
        Ok(snapshot)
    }

    pub(super) fn get(&self, name: &str) -> Option<&EnvValue> {
        self.values.get(&name.to_ascii_uppercase())
    }

    pub(super) fn set(&mut self, value: EnvValue) {
        self.values.insert(value.name.to_ascii_uppercase(), value);
    }

    pub(super) fn remove(&mut self, name: &str) -> Option<EnvValue> {
        self.values.remove(&name.to_ascii_uppercase())
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }
}

#[derive(Clone, Debug)]
pub(super) struct DesiredContext {
    pub(super) id: String,
    pub(super) root: String,
    pub(super) release: String,
    pub(super) path: String,
    pub(super) env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Change {
    pub(super) name: String,
    pub(super) before: Option<EnvValue>,
    pub(super) after: Option<EnvValue>,
}

#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub(super) after: Snapshot,
    pub(super) changes: Vec<Change>,
    pub(super) requires_confirmation: bool,
    pub(super) notes: Vec<String>,
}

impl Plan {
    pub(super) fn between(
        before: Snapshot,
        after: Snapshot,
        requires_confirmation: bool,
        notes: Vec<String>,
    ) -> Self {
        let names = before
            .names()
            .chain(after.names())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let changes = names
            .into_iter()
            .filter_map(|name| {
                let old = before.get(&name).cloned();
                let new = after.get(&name).cloned();
                (old != new).then_some(Change {
                    name,
                    before: old,
                    after: new,
                })
            })
            .collect();
        Self {
            after,
            changes,
            requires_confirmation,
            notes,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct StatusReport {
    pub(super) scope: String,
    pub(super) healthy: bool,
    pub(super) contexts: Vec<ContextStatus>,
    pub(super) warnings: Vec<String>,
    pub(super) issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ContextStatus {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) release: Option<String>,
    pub(super) placement: Option<String>,
    pub(super) path_value: Option<String>,
    pub(super) env: Vec<EnvProviderStatus>,
    pub(super) path_exists: bool,
    pub(super) state: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct EnvProviderStatus {
    pub(super) name: String,
    pub(super) value: String,
    pub(super) active: bool,
}
