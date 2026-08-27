use super::model::{
    DesiredContext, EnvValue, Scope, Snapshot, CONTEXT_ID_LEN, PATH_APPEND, PATH_PREPEND,
};
use super::protocol::{
    context_id_from_normalized, external_windows_path, validate_context_id,
    validate_reference_cycles,
};
use super::reconcile::{plan_apply, plan_remove};
use super::status::build_status;
use std::collections::BTreeMap;

#[test]
fn external_windows_path_removes_verbatim_disk_prefix() {
    assert_eq!(
        external_windows_path(r"\\?\D:\10_Software\Tools\Relo\active\bin"),
        r"D:\10_Software\Tools\Relo\active\bin"
    );
}

#[test]
fn external_windows_path_converts_verbatim_unc_prefix() {
    assert_eq!(
        external_windows_path(r"\\?\UNC\server\share\Relo\active\bin"),
        r"\\server\share\Relo\active\bin"
    );
}

#[test]
fn external_windows_path_preserves_non_verbatim_and_device_paths() {
    assert_eq!(
        external_windows_path(r"D:\Tools\Relo\active\bin"),
        r"D:\Tools\Relo\active\bin"
    );
    assert_eq!(
        external_windows_path(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Relo"),
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Relo"
    );
}

fn desired(id: &str) -> DesiredContext {
    DesiredContext {
        id: id.to_owned(),
        root: format!(r"C:\TOOLS\{id}"),
        release: "1.0.0".to_owned(),
        path: format!(r"C:\TOOLS\{id}\BIN"),
        env: BTreeMap::from([("JAVA_HOME".to_owned(), format!(r"C:\TOOLS\{id}"))]),
    }
}

const A: &str = "00000000000000000000000000";
const B: &str = "11111111111111111111111111";

#[test]
fn context_id_is_stable_and_case_insensitive() {
    let left = context_id_from_normalized(r"C:\TOOLS\MAVEN");
    let right = context_id_from_normalized(r"c:\tools\maven");
    assert_eq!(left, right);
    assert_eq!(left.len(), CONTEXT_ID_LEN);
    validate_context_id(&left).unwrap();
}

#[test]
fn repeated_apply_is_idempotent() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    assert!(!first.is_empty());
    let second = plan_apply(first.after.clone(), &desired(A), false).unwrap();
    assert!(second.is_empty());
}

#[test]
fn latest_apply_wins_env_while_path_keeps_insertion_order() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let second = plan_apply(first.after, &desired(B), false).unwrap();
    assert_eq!(
        second.after.get("JAVA_HOME").unwrap().value,
        format!(r"C:\TOOLS\{B}")
    );
    assert_eq!(second.after.get("RELO_OWNER_JAVA_HOME").unwrap().value, B);
    assert_eq!(
        second.after.get(PATH_PREPEND).unwrap().value,
        format!("%RELO_PATH_{A}%;%RELO_PATH_{B}%")
    );
    let third = plan_apply(second.after, &desired(A), false).unwrap();
    assert_eq!(
        third.after.get("JAVA_HOME").unwrap().value,
        format!(r"C:\TOOLS\{A}")
    );
    assert_eq!(third.after.get("RELO_OWNER_JAVA_HOME").unwrap().value, A);
    assert_eq!(
        third.after.get(PATH_PREPEND).unwrap().value,
        format!("%RELO_PATH_{A}%;%RELO_PATH_{B}%")
    );
}

#[test]
fn removing_winner_deletes_public_env_without_fallback() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let second = plan_apply(first.after, &desired(B), false).unwrap();
    let removed = plan_remove(second.after, B).unwrap();
    assert!(removed.after.get("JAVA_HOME").is_none());
    assert!(removed
        .after
        .get(&format!("RELO_ENV_{A}_JAVA_HOME"))
        .is_some());
    assert!(removed.requires_confirmation);
}

#[test]
fn append_moves_only_the_selected_context() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let second = plan_apply(first.after, &desired(B), false).unwrap();
    let moved = plan_apply(second.after, &desired(A), true).unwrap();
    assert_eq!(
        moved.after.get(PATH_PREPEND).unwrap().value,
        format!("%RELO_PATH_{B}%")
    );
    assert_eq!(
        moved.after.get(PATH_APPEND).unwrap().value,
        format!("%RELO_PATH_{A}%")
    );
}

#[test]
fn config_removal_cleans_stale_provider_and_public_winner() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let mut changed = desired(A);
    changed.env.clear();
    let second = plan_apply(first.after, &changed, false).unwrap();
    assert!(second.after.get("JAVA_HOME").is_none());
    assert!(second
        .after
        .get(&format!("RELO_ENV_{A}_JAVA_HOME"))
        .is_none());
}

#[test]
fn external_reserved_namespace_without_binding_is_rejected() {
    let snapshot =
        Snapshot::from_values([EnvValue::string(format!("RELO_PATH_{A}"), r"C:\unknown")]).unwrap();
    let err = plan_apply(snapshot, &desired(A), false).unwrap_err();
    assert!(err.to_string().contains("cannot verify ownership"));
}

#[test]
fn cycles_are_rejected() {
    let env = BTreeMap::from([
        ("A".to_owned(), "%B%".to_owned()),
        ("B".to_owned(), "%A%".to_owned()),
    ]);
    assert!(validate_reference_cycles(&env).is_err());
}

#[test]
fn first_path_apply_has_no_empty_path_segment() {
    let plan = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    assert_eq!(
        plan.after.get("PATH").unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN")
    );
}

#[test]
fn public_values_do_not_depend_on_same_scope_recursive_expansion() {
    let plan = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    assert_eq!(
        plan.after.get("JAVA_HOME").unwrap().value,
        format!(r"C:\TOOLS\{A}")
    );
    assert!(!plan.after.get("PATH").unwrap().value.contains("%RELO_"));
}

#[test]
fn applying_over_external_env_requires_confirmation() {
    let snapshot = Snapshot::from_values([EnvValue::string("JAVA_HOME", r"C:\manual")]).unwrap();
    let plan = plan_apply(snapshot, &desired(A), false).unwrap();
    assert!(plan.requires_confirmation);
    assert!(plan.notes.iter().any(|note| note.contains("JAVA_HOME")));
}

#[test]
fn applying_over_equal_external_env_still_requires_confirmation() {
    let value = format!(r"C:\TOOLS\{A}");
    let snapshot = Snapshot::from_values([EnvValue::expandable("JAVA_HOME", value)]).unwrap();
    let plan = plan_apply(snapshot, &desired(A), false).unwrap();
    assert!(plan.requires_confirmation);
}

#[test]
fn path_drift_blocks_reconciliation() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let mut snapshot = first.after;
    snapshot.set(EnvValue::expandable("Path", r"C:\manual"));
    let err = plan_apply(snapshot, &desired(B), false).unwrap_err();
    assert!(err.to_string().contains("refusing to overwrite"));
}

#[test]
fn unknown_reserved_state_is_rejected() {
    let snapshot = Snapshot::from_values([EnvValue::string("RELO_UNKNOWN", "value")]).unwrap();
    let err = plan_apply(snapshot, &desired(A), false).unwrap_err();
    assert!(err
        .to_string()
        .contains("unrecognized reserved environment variable"));
}

#[test]
fn status_reports_external_drift_and_dormant_providers() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let mut snapshot = first.after;
    snapshot.set(EnvValue::string("JAVA_HOME", r"C:\manual"));
    let report = build_status(&snapshot, Scope::User, None);
    assert!(!report.healthy);
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("public value was modified")));
    assert!(!report.contexts[0].env[0].active);
}
