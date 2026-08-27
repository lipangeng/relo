use super::model::*;
use super::protocol::*;
use crate::layout::Layout;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

pub(super) fn desired_context(
    layout: &Layout,
    logical_root: &Path,
    release: &str,
) -> Result<DesiredContext> {
    let root = normalize_context_path(logical_root)?;
    let id = context_id_from_normalized(&root);
    let mut env = BTreeMap::new();
    for (name, value) in layout.effective_env(release)? {
        let normalized = normalize_name(&name);
        if normalized.starts_with("RELO_") {
            bail!("RELO_ is reserved for persistent environment management: {name}");
        }
        if let Some(previous) = env.insert(normalized.clone(), value) {
            bail!(
                "Windows environment variable names are case-insensitive; duplicate variable {name} conflicts with value {previous:?}"
            );
        }
    }
    validate_reference_cycles(&env)?;
    let path = layout
        .effective_path(release, &[])?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(";");
    validate_value_length(&path, "PATH")?;
    for (name, value) in &env {
        validate_value_length(value, name)?;
    }
    Ok(DesiredContext {
        id,
        root,
        release: release.to_owned(),
        path,
        env,
    })
}

pub(super) fn plan_apply(
    before: Snapshot,
    desired: &DesiredContext,
    path_append: bool,
) -> Result<Plan> {
    validate_context_id(&desired.id)?;
    validate_value_length(&desired.root, "context path")?;
    validate_value_length(&desired.release, "release id")?;
    validate_protocol(&before)?;
    let mut after = before.clone();
    let context_name = format!("{CONTEXT_PREFIX}{}", desired.id);
    let release_name = format!("{RELEASE_PREFIX}{}", desired.id);
    let path_name = context_path_name(&desired.id);

    match before.get(&context_name) {
        Some(binding) if !windows_path_eq(&binding.value, &desired.root) => {
            bail!(
                "context id collision: {} is already bound to {}",
                desired.id,
                binding.value
            );
        }
        None if has_context_namespace(&before, &desired.id) => {
            bail!(
                "reserved RELO_ state exists for {} without a verifiable context binding",
                desired.id
            );
        }
        _ => {}
    }

    after.set(EnvValue::string(&context_name, &desired.root));
    after.set(EnvValue::string(&release_name, &desired.release));

    let token = path_reference(&desired.id);
    let mut prepend = aggregate_references(&before, PATH_PREPEND);
    let mut append = aggregate_references(&before, PATH_APPEND);
    let old_prepend = remove_reference(&mut prepend, &token);
    let old_append = remove_reference(&mut append, &token);
    if desired.path.is_empty() {
        after.remove(&path_name);
    } else {
        after.set(EnvValue::expandable(&path_name, &desired.path));
        if path_append {
            append.push(token);
        } else if let Some(index) = old_append {
            append.insert(index.min(append.len()), token);
        } else if let Some(index) = old_prepend {
            prepend.insert(index.min(prepend.len()), token);
        } else {
            prepend.push(token);
        }
    }

    let provider_prefix = format!("{ENV_PREFIX}{}_", desired.id);
    let desired_provider_names = desired
        .env
        .keys()
        .map(|name| format!("{provider_prefix}{name}"))
        .collect::<BTreeSet<_>>();
    let stale = before
        .names()
        .filter(|name| name.starts_with(&provider_prefix))
        .filter(|name| !desired_provider_names.contains(*name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for provider in stale {
        let public_name = provider.trim_start_matches(&provider_prefix);
        if after
            .get(public_name)
            .is_some_and(|value| value.value.eq_ignore_ascii_case(&reference(&provider)))
        {
            after.remove(public_name);
        }
        after.remove(&provider);
    }
    let dangling_current_refs = before
        .values
        .values()
        .filter(|value| {
            value.value.starts_with(&format!("%{provider_prefix}"))
                && value.value.ends_with('%')
                && before.get(value.value.trim_matches('%')).is_none()
        })
        .map(|value| value.name.clone())
        .collect::<Vec<_>>();
    for public_name in dangling_current_refs {
        if !desired.env.contains_key(&normalize_name(&public_name)) {
            after.remove(&public_name);
        }
    }

    let mut requires_confirmation = false;
    let mut notes = Vec::new();
    for (name, value) in &desired.env {
        let provider = format!("{provider_prefix}{name}");
        let provider_ref = reference(&provider);
        after.set(EnvValue::expandable(&provider, value));
        if let Some(current) = before.get(name) {
            if !is_relo_provider_reference(&current.value) && current.value != provider_ref {
                requires_confirmation = true;
                notes.push(format!(
                    "{name} is externally managed and will not be restored after removal"
                ));
            }
        }
        after.set(EnvValue::expandable(name, provider_ref));
    }

    write_aggregates_and_path(&mut after, prepend, append)?;
    if before.get("PATH").is_some_and(|value| {
        value.kind == ValueKind::String
            && after
                .get("PATH")
                .is_some_and(|next| next.kind == ValueKind::ExpandString)
    }) {
        requires_confirmation = true;
        notes.push("Path registry type will change from REG_SZ to REG_EXPAND_SZ".to_owned());
    }
    validate_snapshot_size(&after)?;
    Ok(Plan::between(before, after, requires_confirmation, notes))
}

pub(super) fn plan_remove(before: Snapshot, id: &str) -> Result<Plan> {
    validate_context_id(id)?;
    validate_protocol(&before)?;
    let binding = format!("{CONTEXT_PREFIX}{id}");
    if before.get(&binding).is_none() {
        if has_context_namespace(&before, id) {
            bail!("cannot verify ownership for context id {id}");
        }
        bail!("context id is not applied: {id}");
    }
    let mut after = before.clone();
    let provider_prefix = format!("{ENV_PREFIX}{id}_");
    let providers = before
        .names()
        .filter(|name| name.starts_with(&provider_prefix))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut requires_confirmation = false;
    let mut notes = Vec::new();
    for provider in &providers {
        let public_name = provider.trim_start_matches(&provider_prefix);
        if after
            .get(public_name)
            .is_some_and(|value| value.value.eq_ignore_ascii_case(&reference(provider)))
        {
            after.remove(public_name);
            requires_confirmation = true;
            notes.push(format!(
                "{public_name} has no automatic fallback and will be deleted"
            ));
        }
        after.remove(provider);
    }
    after.remove(&binding);
    after.remove(&format!("{RELEASE_PREFIX}{id}"));
    after.remove(&context_path_name(id));

    let token = path_reference(id);
    let mut prepend = aggregate_references(&before, PATH_PREPEND);
    let mut append = aggregate_references(&before, PATH_APPEND);
    remove_reference(&mut prepend, &token);
    remove_reference(&mut append, &token);
    write_aggregates_and_path(&mut after, prepend, append)?;
    validate_snapshot_size(&after)?;
    Ok(Plan::between(before, after, requires_confirmation, notes))
}

pub(super) fn plan_prune(before: Snapshot) -> Result<Plan> {
    validate_protocol(&before)?;
    let mut current = before.clone();
    let ids = discover_contexts(&before)
        .into_iter()
        .filter_map(|(id, path)| match std::fs::metadata(&path) {
            Ok(_) => None,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Some(Ok(id)),
            Err(err) => Some(Err(err).with_context(|| format!("cannot inspect {path}"))),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut requires_confirmation = false;
    let mut notes = Vec::new();
    for id in ids {
        let plan = plan_remove(current, &id)?;
        current = plan.after;
        requires_confirmation |= plan.requires_confirmation;
        notes.push(format!("pruning missing context {id}"));
        notes.extend(plan.notes);
    }
    validate_snapshot_size(&current)?;
    Ok(Plan::between(before, current, requires_confirmation, notes))
}

fn write_aggregates_and_path(
    snapshot: &mut Snapshot,
    prepend: Vec<String>,
    append: Vec<String>,
) -> Result<()> {
    let prepend_value = prepend.join(";");
    let append_value = append.join(";");
    if prepend.is_empty() {
        snapshot.remove(PATH_PREPEND);
    } else {
        validate_value_length(&prepend_value, PATH_PREPEND)?;
        snapshot.set(EnvValue::expandable(PATH_PREPEND, prepend_value));
    }
    if append.is_empty() {
        snapshot.remove(PATH_APPEND);
    } else {
        validate_value_length(&append_value, PATH_APPEND)?;
        snapshot.set(EnvValue::expandable(PATH_APPEND, append_value));
    }

    let had_path = snapshot.get("PATH").is_some();
    let existing = snapshot
        .get("PATH")
        .map(|value| value.value.clone())
        .unwrap_or_default();
    let mut segments = if had_path {
        existing
            .split(';')
            .filter(|segment| !segment.eq_ignore_ascii_case(&reference(PATH_PREPEND)))
            .filter(|segment| !segment.eq_ignore_ascii_case(&reference(PATH_APPEND)))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !prepend.is_empty() {
        segments.insert(0, reference(PATH_PREPEND));
    }
    if !append.is_empty() {
        segments.push(reference(PATH_APPEND));
    }
    let value = segments.join(";");
    validate_value_length(&value, "Path")?;
    if value.is_empty() {
        snapshot.remove("PATH");
    } else {
        let kind = if prepend.is_empty() && append.is_empty() {
            snapshot
                .get("PATH")
                .map(|value| value.kind)
                .unwrap_or(ValueKind::ExpandString)
        } else {
            ValueKind::ExpandString
        };
        snapshot.set(EnvValue {
            name: "Path".to_owned(),
            value,
            kind,
        });
    }
    Ok(())
}
