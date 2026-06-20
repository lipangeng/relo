use crate::error::ReloError;
use anyhow::{bail, Result};
use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq)]
pub struct Release {
    pub id: String,
    pub version: DottedVersion,
    pub label: Option<String>,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DottedVersion {
    parts: Vec<u64>,
}

impl DottedVersion {
    fn parse(value: &str) -> Result<Self> {
        let value = value
            .strip_prefix('v')
            .or_else(|| value.strip_prefix('V'))
            .unwrap_or(value);
        let parts = value
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
                    bail!("invalid dotted version");
                }
                part.parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("invalid dotted version"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { parts })
    }

    fn starts_with(&self, prefix: &DottedVersion) -> bool {
        self.parts.starts_with(&prefix.parts)
    }
}

impl Ord for DottedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl PartialOrd for DottedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Release {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        // Sorting is numeric-version-first so 3.10.0 is newer than 3.9.9,
        // and 8.1.1.10 is newer than 8.1.1.7.
        // The release id is only a stable tie-breaker for labeled variants.
        self.version
            .cmp(&other.version)
            .then_with(|| self.label.is_some().cmp(&other.label.is_some()))
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn parse_release(id: &str, path: PathBuf) -> Result<Release> {
    // Release directory names keep the full id, but version matching only uses
    // the dotted version part before the separator: 3.9.9_arm64 -> 3.9.9 + arm64.
    let (version_text, label) = match id.split_once('_') {
        Some((version_text, label)) if !label.is_empty() => (version_text, Some(label.to_string())),
        Some(_) => return Err(ReloError::InvalidRelease(id.to_string()).into()),
        None => (id, None),
    };
    let version = DottedVersion::parse(version_text)
        .map_err(|_| ReloError::InvalidRelease(id.to_string()))?;
    Ok(Release {
        id: id.to_string(),
        version,
        label,
        path,
    })
}

pub fn resolve(releases: &[Release], expr: &str) -> Result<Release> {
    if releases.is_empty() {
        return Err(ReloError::NoMatch(expr.to_string()).into());
    }

    if expr == "latest" {
        return Ok(releases.iter().max().unwrap().clone());
    }

    if let Some(exact) = releases.iter().find(|release| release.id == expr) {
        return Ok(exact.clone());
    }

    let version_expr =
        DottedVersion::parse(expr).map_err(|_| ReloError::NoMatch(expr.to_string()))?;

    let matches: Vec<&Release> = releases
        .iter()
        .filter(|release| release.version.starts_with(&version_expr))
        .collect();

    if matches.is_empty() {
        return Err(ReloError::NoMatch(expr.to_string()).into());
    }

    let exact_version_matches = matches
        .iter()
        .copied()
        .filter(|release| release.version == version_expr)
        .collect::<Vec<_>>();
    if !exact_version_matches.is_empty() {
        // A bare exact version expression prefers the unlabeled directory.
        // If only labeled variants exist, choosing one would be arbitrary.
        if let Some(unlabeled) = exact_version_matches
            .iter()
            .find(|release| release.label.is_none())
        {
            return Ok((*unlabeled).clone());
        }
        if exact_version_matches.len() > 1 {
            let mut exact_version_matches = exact_version_matches;
            exact_version_matches.sort();
            let lines = exact_version_matches
                .iter()
                .map(|release| format!("  {}", release.id))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(ReloError::Ambiguous {
                expr: expr.to_string(),
                matches: lines,
            }
            .into());
        }
    }

    Ok(matches.into_iter().max().unwrap().clone())
}
