use super::model::*;
use super::protocol::*;
use super::reconcile::desired_context;
use crate::layout::Layout;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn build_status(
    snapshot: &Snapshot,
    scope: Scope,
    selected: Option<&str>,
) -> StatusReport {
    let prepend =
        configured_path_ids(snapshot, CONF_PATH_PREPEND, PATH_PREPEND).unwrap_or_default();
    let append = configured_path_ids(snapshot, CONF_PATH_APPEND, PATH_APPEND).unwrap_or_default();
    let mut issues = Vec::new();
    if let Err(error) = validate_protocol(snapshot) {
        issues.push(format!("protocol conflict: {error:#}"));
    }
    validate_path_layout(snapshot, &prepend, &append, &mut issues);

    let mut contexts = discover_contexts(snapshot)
        .into_iter()
        .filter(|(id, _)| selected.is_none_or(|selected| selected == id))
        .map(|(id, path)| {
            let placement = if prepend.iter().any(|item| item.eq_ignore_ascii_case(&id)) {
                Some("prepend".to_owned())
            } else if append.iter().any(|item| item.eq_ignore_ascii_case(&id)) {
                Some("append".to_owned())
            } else {
                None
            };
            let release = snapshot
                .get(&format!("{RELEASE_PREFIX}{id}"))
                .map(|value| value.value.clone());
            let path_value = snapshot
                .get(&context_path_name(&id))
                .map(|value| value.value.clone());
            let provider_prefix = format!("{ENV_PREFIX}{id}_");
            let env = snapshot
                .values
                .iter()
                .filter_map(|(provider, value)| {
                    let name = provider.strip_prefix(&provider_prefix)?;
                    Some(EnvProviderStatus {
                        name: name.to_owned(),
                        value: value.value.clone(),
                        active: owner_id(snapshot, name) == Some(id.as_str())
                            && provider_matches_public(snapshot, &id, name),
                    })
                })
                .collect::<Vec<_>>();
            let context_path = Path::new(&path);
            let path_exists = context_path.exists();
            let provider_exists = snapshot.get(&context_path_name(&id)).is_some();
            let mut state = if !path_exists {
                "orphaned".to_owned()
            } else if release.is_none() || provider_exists != placement.is_some() {
                "drifted".to_owned()
            } else {
                "healthy".to_owned()
            };
            if path_exists {
                let resolved_path = std::fs::canonicalize(context_path)
                    .unwrap_or_else(|_| context_path.to_path_buf());
                match Layout::load(resolved_path) {
                    Ok(layout) => {
                        if let Some(applied_release) = release.as_deref() {
                            if layout.resolve(applied_release).is_err() {
                                state = "drifted".to_owned();
                            }
                            match layout.active_version() {
                                Ok(active) if active.as_deref() != Some(applied_release) => {
                                    state = "stale".to_owned();
                                }
                                Err(_) => state = "drifted".to_owned(),
                                _ => {}
                            }
                            match desired_context(&layout, context_path, applied_release) {
                                Ok(desired) => {
                                    if desired.id != id || !matches_desired(snapshot, &desired) {
                                        state = "drifted".to_owned();
                                    }
                                }
                                Err(_) => state = "drifted".to_owned(),
                            }
                        }
                    }
                    Err(_) => state = "drifted".to_owned(),
                }
            }
            if state != "healthy" {
                issues.push(format!("context {id} is {state}"));
            }
            ContextStatus {
                id,
                path,
                release,
                placement,
                path_value,
                env,
                path_exists,
                state,
            }
        })
        .collect::<Vec<_>>();
    contexts.sort_by(|left, right| {
        context_status_rank(left, &prepend, &append)
            .cmp(&context_status_rank(right, &prepend, &append))
            .then_with(|| left.path.cmp(&right.path))
    });
    if let Some(id) = selected {
        if contexts.is_empty() {
            issues.push(format!("context {id} is not applied"));
        }
    }
    for value in snapshot.values.values() {
        if is_relo_provider_reference(&value.value) {
            let provider = value.value.trim_matches('%');
            if snapshot.get(provider).is_none() {
                issues.push(format!(
                    "{} references missing provider {provider}",
                    value.name
                ));
            }
        }
    }
    for owner in snapshot
        .names()
        .filter_map(|name| name.strip_prefix(OWNER_PREFIX))
    {
        let Some(id) = owner_id(snapshot, owner) else {
            continue;
        };
        if !provider_matches_public(snapshot, id, owner) {
            issues.push(format!(
                "{owner} has a relo owner marker but its public value was modified"
            ));
        }
    }
    StatusReport {
        scope: scope.as_str().to_owned(),
        healthy: issues.is_empty(),
        contexts,
        warnings: Vec::new(),
        issues,
    }
}

pub(super) fn scope_shadow_warnings(system: &Snapshot, user: &Snapshot) -> Vec<String> {
    system
        .names()
        .filter_map(|name| name.strip_prefix(OWNER_PREFIX))
        .filter_map(|name| system.get(name))
        .filter_map(|system_value| {
            user.get(&system_value.name)
                .filter(|user_value| user_value != &system_value)
                .map(|user_value| {
                    format!(
                        "system {} is shadowed by user value {}",
                        system_value.name, user_value.value
                    )
                })
        })
        .collect()
}

fn matches_desired(snapshot: &Snapshot, desired: &DesiredContext) -> bool {
    let path_matches = if desired.path.is_empty() {
        snapshot.get(&context_path_name(&desired.id)).is_none()
    } else {
        snapshot
            .get(&context_path_name(&desired.id))
            .is_some_and(|value| value.value == desired.path)
    };
    if !path_matches {
        return false;
    }
    let prefix = format!("{ENV_PREFIX}{}_", desired.id);
    let actual = snapshot
        .values
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix(&prefix)
                .map(|name| (name.to_owned(), value.value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    actual == desired.env
}

fn validate_path_layout(
    snapshot: &Snapshot,
    prepend_references: &[String],
    append_references: &[String],
    issues: &mut Vec<String>,
) {
    let segments = snapshot
        .get("PATH")
        .map(|value| value.value.split(';').collect::<Vec<_>>())
        .unwrap_or_default();
    let prepend = if snapshot.get(CONF_PATH_PREPEND).is_some() && !prepend_references.is_empty() {
        vec![reference(PATH_PREPEND)]
    } else {
        path_segments_for_ids(snapshot, prepend_references, issues)
    };
    let append = if snapshot.get(CONF_PATH_APPEND).is_some() && !append_references.is_empty() {
        vec![reference(PATH_APPEND)]
    } else {
        path_segments_for_ids(snapshot, append_references, issues)
    };
    if segments.len() < prepend.len() + append.len()
        || !segments
            .iter()
            .zip(&prepend)
            .all(|(actual, expected)| windows_path_eq(actual, expected))
        || !segments[segments.len().saturating_sub(append.len())..]
            .iter()
            .zip(&append)
            .all(|(actual, expected)| windows_path_eq(actual, expected))
    {
        issues.push(
            "Path no longer contains relo-managed entries at their recorded positions".to_owned(),
        );
    }
}

fn path_segments_for_ids(
    snapshot: &Snapshot,
    ids: &[String],
    issues: &mut Vec<String>,
) -> Vec<String> {
    let mut segments = Vec::new();
    for id in ids {
        let provider = context_path_name(id);
        let Some(value) = snapshot.get(&provider) else {
            issues.push(format!("missing managed PATH provider {provider}"));
            continue;
        };
        segments.extend(
            value
                .value
                .split(';')
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned),
        );
    }
    segments
}

pub(super) fn print_status(report: &StatusReport) {
    println!("scope:   {}", report.scope);
    println!(
        "status:  {}",
        if report.healthy { "healthy" } else { "drifted" }
    );
    println!("contexts:");
    if report.contexts.is_empty() {
        println!("  (none)");
    }
    for context in &report.contexts {
        println!("  {} {}", context.id, context.path);
        println!(
            "    release: {}",
            context.release.as_deref().unwrap_or("missing")
        );
        println!(
            "    path:    {}",
            context.placement.as_deref().unwrap_or("none")
        );
        if let Some(value) = &context.path_value {
            println!("    paths:   {value}");
        }
        println!("    env:");
        if context.env.is_empty() {
            println!("      (none)");
        }
        for variable in &context.env {
            println!(
                "      {}={}{}",
                variable.name,
                variable.value,
                if variable.active {
                    " (active)"
                } else {
                    " (dormant)"
                }
            );
        }
        println!("    state:   {}", context.state);
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for issue in &report.issues {
        println!("issue:   {issue}");
    }
}
