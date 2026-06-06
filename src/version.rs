use crate::error::ReloError;
use anyhow::Result;
use semver::Version;
use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq)]
pub struct Release {
    pub id: String,
    pub semver: Version,
    pub label: Option<String>,
    pub path: PathBuf,
}

impl PartialEq for Release {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        // Sorting is semver-first so 3.10.0 is newer than 3.9.9.
        // The release id is only a stable tie-breaker for labeled variants.
        self.semver
            .cmp(&other.semver)
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
    // the semver part before the separator: 3.9.9_arm64 -> 3.9.9 + arm64.
    let (version, label) = match id.split_once('_') {
        Some((version, label)) if !label.is_empty() => (version, Some(label.to_string())),
        Some(_) => return Err(ReloError::InvalidRelease(id.to_string()).into()),
        None => (id, None),
    };
    let semver = Version::parse(version).map_err(|_| ReloError::InvalidRelease(id.to_string()))?;
    Ok(Release {
        id: id.to_string(),
        semver,
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

    let parts: Vec<&str> = expr.split('.').collect();
    if !(1..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(ReloError::NoMatch(expr.to_string()).into());
    }

    let mut matches: Vec<&Release> = releases
        .iter()
        .filter(|release| match parts.len() {
            1 => release.semver.major.to_string() == parts[0],
            2 => {
                release.semver.major.to_string() == parts[0]
                    && release.semver.minor.to_string() == parts[1]
            }
            3 => {
                release.semver.major.to_string() == parts[0]
                    && release.semver.minor.to_string() == parts[1]
                    && release.semver.patch.to_string() == parts[2]
            }
            _ => false,
        })
        .collect();

    if matches.is_empty() {
        return Err(ReloError::NoMatch(expr.to_string()).into());
    }

    if parts.len() == 3 {
        // A bare exact semver expression prefers the unlabeled directory.
        // If only labeled variants exist, choosing one would be arbitrary.
        if let Some(unlabeled) = matches.iter().find(|release| release.label.is_none()) {
            return Ok((*unlabeled).clone());
        }
        if matches.len() > 1 {
            matches.sort();
            let lines = matches
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
