use super::model::*;
use crate::paths;
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub(super) fn normalize_context_path(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    let normalized_value = normalized.display().to_string().replace('/', "\\");
    let value = paths::external_windows(&normalized_value);
    if normalized.parent().is_none() {
        Ok(value.into_owned())
    } else {
        Ok(value.trim_end_matches('\\').to_owned())
    }
}

pub(super) fn context_id(path: &Path) -> Result<String> {
    Ok(context_id_from_normalized(&normalize_context_path(path)?))
}

pub(super) fn context_id_from_normalized(path: &str) -> String {
    let digest = Sha256::digest(path.to_uppercase().as_bytes());
    crockford_base32(&digest[..16])
}

fn crockford_base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

pub(super) fn validate_context_id(id: &str) -> Result<()> {
    if id.len() != CONTEXT_ID_LEN
        || !id
            .bytes()
            .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte))
    {
        bail!("invalid context id: {id}");
    }
    Ok(())
}

pub(super) fn discover_contexts(snapshot: &Snapshot) -> Vec<(String, String)> {
    snapshot
        .values
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix(CONTEXT_PREFIX)
                .filter(|id| validate_context_id(id).is_ok())
                .map(|id| (id.to_owned(), value.value.clone()))
        })
        .collect()
}

pub(super) fn has_context_namespace(snapshot: &Snapshot, id: &str) -> bool {
    let exact = [
        format!("{CONTEXT_PREFIX}{id}"),
        format!("{RELEASE_PREFIX}{id}"),
        context_path_name(id),
    ];
    let env_prefix = format!("{ENV_PREFIX}{id}_");
    snapshot
        .names()
        .any(|name| exact.iter().any(|exact| name == exact) || name.starts_with(&env_prefix))
}

pub(super) fn configured_path_ids(
    snapshot: &Snapshot,
    config_name: &str,
    aggregate_name: &str,
) -> Result<Vec<String>> {
    if let Some(config) = snapshot.get(config_name) {
        return Ok(config
            .value
            .split(';')
            .filter(|id| !id.is_empty())
            .map(normalize_name)
            .collect());
    }
    legacy_aggregate_ids(snapshot, aggregate_name)
}

fn legacy_aggregate_ids(snapshot: &Snapshot, name: &str) -> Result<Vec<String>> {
    snapshot
        .get(name)
        .map(|value| {
            value
                .value
                .split(';')
                .filter(|token| !token.is_empty())
                .map(|token| {
                    token
                        .strip_prefix('%')
                        .and_then(|token| token.strip_suffix('%'))
                        .and_then(|provider| provider.strip_prefix(PATH_PREFIX))
                        .map(normalize_name)
                        .ok_or_else(|| {
                            anyhow::anyhow!("{name} contains a non-relo path entry: {token}")
                        })
                })
                .collect()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn materialized_path_value(snapshot: &Snapshot, ids: &[String]) -> Result<String> {
    let mut segments = Vec::new();
    for id in ids {
        let provider = context_path_name(id);
        let value = snapshot
            .get(&provider)
            .ok_or_else(|| anyhow::anyhow!("missing managed PATH provider {provider}"))?;
        segments.extend(
            value
                .value
                .split(';')
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned),
        );
    }
    Ok(segments.join(";"))
}

pub(super) fn validate_protocol(snapshot: &Snapshot) -> Result<()> {
    for name in snapshot.names().filter(|name| name.starts_with("RELO_")) {
        if matches!(
            name,
            PATH_PREPEND | PATH_APPEND | CONF_PATH_PREPEND | CONF_PATH_APPEND
        ) {
            continue;
        }
        let id = if let Some(variable) = name.strip_prefix(OWNER_PREFIX) {
            if variable.is_empty() {
                bail!("unrecognized reserved environment variable: {name}");
            }
            let owner = &snapshot.get(name).expect("name came from snapshot").value;
            require_binding(snapshot, owner, name)?;
            let provider = provider_name(owner, variable);
            if snapshot.get(&provider).is_none() {
                bail!("cannot verify ownership of {variable}: missing {provider}");
            }
            continue;
        } else if let Some(id) = name.strip_prefix(CONTEXT_PREFIX) {
            id
        } else if let Some(id) = name.strip_prefix(RELEASE_PREFIX) {
            require_binding(snapshot, id, name)?;
            id
        } else if let Some(id) = name.strip_prefix(PATH_PREFIX) {
            require_binding(snapshot, id, name)?;
            id
        } else if let Some(rest) = name.strip_prefix(ENV_PREFIX) {
            let Some((id, variable)) = rest.split_once('_') else {
                bail!("unrecognized reserved environment variable: {name}");
            };
            if variable.is_empty() {
                bail!("unrecognized reserved environment variable: {name}");
            }
            require_binding(snapshot, id, name)?;
            id
        } else {
            bail!("unrecognized reserved environment variable: {name}");
        };
        validate_context_id(id)?;
    }

    for (config, aggregate) in [
        (CONF_PATH_PREPEND, PATH_PREPEND),
        (CONF_PATH_APPEND, PATH_APPEND),
    ] {
        let ids = configured_path_ids(snapshot, config, aggregate)?;
        let mut seen = BTreeSet::new();
        for id in &ids {
            validate_context_id(id)?;
            require_binding(snapshot, id, config)?;
            if !seen.insert(id) {
                bail!("{config} contains duplicate context id {id}");
            }
            if snapshot.get(&context_path_name(id)).is_none() {
                bail!("{config} references missing PATH provider {PATH_PREFIX}{id}");
            }
        }
        if snapshot.get(config).is_some() {
            if ids.is_empty() {
                bail!("{config} must not be empty");
            }
            let expected = materialized_path_value(snapshot, &ids)?;
            let Some(actual) = snapshot.get(aggregate) else {
                bail!("{config} exists but {aggregate} is missing");
            };
            if actual.value != expected {
                bail!("{aggregate} does not match the context order in {config}");
            }
        }
    }
    Ok(())
}

fn require_binding(snapshot: &Snapshot, id: &str, source: &str) -> Result<()> {
    validate_context_id(id)?;
    if snapshot.get(&format!("{CONTEXT_PREFIX}{id}")).is_none() {
        bail!("cannot verify ownership of {source}: missing {CONTEXT_PREFIX}{id}");
    }
    Ok(())
}

pub(super) fn remove_reference(references: &mut Vec<String>, target: &str) -> Option<usize> {
    let first = references
        .iter()
        .position(|item| item.eq_ignore_ascii_case(target));
    references.retain(|item| !item.eq_ignore_ascii_case(target));
    first
}

pub(super) fn context_path_name(id: &str) -> String {
    format!("{PATH_PREFIX}{id}")
}

pub(super) fn reference(name: &str) -> String {
    format!("%{name}%")
}

pub(super) fn is_relo_provider_reference(value: &str) -> bool {
    let Some(name) = value
        .strip_prefix('%')
        .and_then(|value| value.strip_suffix('%'))
    else {
        return false;
    };
    name.starts_with(ENV_PREFIX)
}

pub(super) fn provider_name(id: &str, name: &str) -> String {
    format!("{ENV_PREFIX}{id}_{name}")
}

pub(super) fn owner_name(name: &str) -> String {
    format!("{OWNER_PREFIX}{name}")
}

pub(super) fn owner_id<'a>(snapshot: &'a Snapshot, name: &str) -> Option<&'a str> {
    snapshot
        .get(&owner_name(name))
        .map(|value| value.value.as_str())
}

pub(super) fn provider_matches_public(snapshot: &Snapshot, id: &str, name: &str) -> bool {
    let Some(provider) = snapshot.get(&provider_name(id, name)) else {
        return false;
    };
    snapshot
        .get(name)
        .is_some_and(|public| public.value == provider.value && public.kind == provider.kind)
}

pub(super) fn normalize_name(name: &str) -> String {
    name.to_ascii_uppercase()
}

pub(super) fn windows_path_eq(left: &str, right: &str) -> bool {
    left.trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_uppercase()
        == right
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_uppercase()
}

pub(super) fn is_under_user_profile(path: &str) -> bool {
    std::env::var("USERPROFILE")
        .ok()
        .and_then(|profile| normalize_context_path(Path::new(&profile)).ok())
        .is_some_and(|profile| {
            let path = path.to_uppercase();
            let profile = profile.to_uppercase();
            path == profile || path.starts_with(&(profile + "\\"))
        })
}

pub(super) fn validate_value_length(value: &str, name: &str) -> Result<()> {
    if value.encode_utf16().count() >= MAX_ENV_VALUE_LEN {
        bail!("Windows environment variable {name} exceeds 32,767 UTF-16 code units");
    }
    Ok(())
}

pub(super) fn validate_snapshot_size(snapshot: &Snapshot) -> Result<()> {
    let size = snapshot.values.values().fold(1usize, |size, value| {
        size + value.name.encode_utf16().count() + 1 + value.value.encode_utf16().count() + 1
    });
    if size >= MAX_ENV_VALUE_LEN {
        bail!("Windows persistent environment block would exceed 32,767 UTF-16 code units");
    }
    Ok(())
}

pub(super) fn validate_reference_cycles(env: &BTreeMap<String, String>) -> Result<()> {
    fn visit(
        name: &str,
        env: &BTreeMap<String, String>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_owned()) {
            bail!("cyclic Windows environment reference involving {name}");
        }
        if let Some(value) = env.get(name) {
            for referenced in percent_references(value) {
                let referenced = normalize_name(&referenced);
                if env.contains_key(&referenced) {
                    visit(&referenced, env, visiting, visited)?;
                }
            }
        }
        visiting.remove(name);
        visited.insert(name.to_owned());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for name in env.keys() {
        visit(name, env, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn percent_references(value: &str) -> Vec<String> {
    let mut references = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('%') else {
            break;
        };
        if end > 0 {
            references.push(rest[..end].to_owned());
        }
        rest = &rest[end + 1..];
    }
    references
}

pub(super) fn context_status_rank(
    context: &ContextStatus,
    prepend: &[String],
    append: &[String],
) -> (u8, usize) {
    if let Some(index) = prepend
        .iter()
        .position(|entry| entry.eq_ignore_ascii_case(&context.id))
    {
        (0, index)
    } else if let Some(index) = append
        .iter()
        .position(|entry| entry.eq_ignore_ascii_case(&context.id))
    {
        (1, index)
    } else {
        (2, 0)
    }
}
