use super::model::{
    DesiredContext, EnvValue, Scope, Snapshot, ValueKind, CONF_PATH_APPEND, CONF_PATH_PREPEND,
    CONTEXT_ID_LEN, PATH_APPEND, PATH_PREPEND,
};
use super::protocol::{context_id_from_normalized, validate_context_id, validate_reference_cycles};
use super::reconcile::{plan_apply, plan_remove};
use super::status::build_status;
use std::collections::BTreeMap;

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
        second.after.get(CONF_PATH_PREPEND).unwrap().value,
        format!("{A};{B}")
    );
    assert_eq!(
        second.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN;C:\TOOLS\{B}\BIN")
    );
    assert_eq!(
        second.after.get("PATH").unwrap().value,
        "%RELO_PATH_PREPEND%"
    );
    let third = plan_apply(second.after, &desired(A), false).unwrap();
    assert_eq!(
        third.after.get("JAVA_HOME").unwrap().value,
        format!(r"C:\TOOLS\{A}")
    );
    assert_eq!(third.after.get("RELO_OWNER_JAVA_HOME").unwrap().value, A);
    assert_eq!(
        third.after.get(CONF_PATH_PREPEND).unwrap().value,
        format!("{A};{B}")
    );
    assert_eq!(
        third.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN;C:\TOOLS\{B}\BIN")
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
    assert_eq!(moved.after.get(CONF_PATH_PREPEND).unwrap().value, B);
    assert_eq!(
        moved.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{B}\BIN")
    );
    assert_eq!(moved.after.get(CONF_PATH_APPEND).unwrap().value, A);
    assert_eq!(
        moved.after.get(PATH_APPEND).unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN")
    );
    assert_eq!(
        moved.after.get("PATH").unwrap().value,
        "%RELO_PATH_PREPEND%;%RELO_PATH_APPEND%"
    );
}

#[test]
fn legacy_reference_aggregates_migrate_to_config_and_concrete_values() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let mut legacy = first.after;
    legacy.remove(CONF_PATH_PREPEND);
    legacy.set(EnvValue::expandable(
        PATH_PREPEND,
        format!("%RELO_PATH_{A}%"),
    ));
    legacy.set(EnvValue::expandable("Path", format!(r"C:\TOOLS\{A}\BIN")));

    let migrated = plan_apply(legacy, &desired(A), false).unwrap();
    assert_eq!(migrated.after.get(CONF_PATH_PREPEND).unwrap().value, A);
    assert_eq!(
        migrated.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN")
    );
    assert_eq!(
        migrated.after.get("Path").unwrap().value,
        "%RELO_PATH_PREPEND%"
    );
}

#[test]
fn removing_context_rebuilds_config_aggregate_and_path_anchor() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let second = plan_apply(first.after, &desired(B), false).unwrap();
    let removed_a = plan_remove(second.after, A).unwrap();
    assert_eq!(removed_a.after.get(CONF_PATH_PREPEND).unwrap().value, B);
    assert_eq!(
        removed_a.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{B}\BIN")
    );
    assert_eq!(
        removed_a.after.get("Path").unwrap().value,
        "%RELO_PATH_PREPEND%"
    );

    let removed_b = plan_remove(removed_a.after, B).unwrap();
    assert!(removed_b.after.get(CONF_PATH_PREPEND).is_none());
    assert!(removed_b.after.get(PATH_PREPEND).is_none());
    assert!(removed_b.after.get("Path").is_none());
}

#[test]
fn external_path_is_preserved_between_relo_anchors() {
    let snapshot =
        Snapshot::from_values([EnvValue::expandable("Path", r"C:\Windows\System32")]).unwrap();
    let applied = plan_apply(snapshot, &desired(A), false).unwrap();
    assert_eq!(
        applied.after.get("Path").unwrap().value,
        r"%RELO_PATH_PREPEND%;C:\Windows\System32"
    );
    let removed = plan_remove(applied.after, A).unwrap();
    assert_eq!(
        removed.after.get("Path").unwrap().value,
        r"C:\Windows\System32"
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
    assert_eq!(plan.after.get(CONF_PATH_PREPEND).unwrap().value, A);
    assert_eq!(
        plan.after.get(CONF_PATH_PREPEND).unwrap().kind,
        ValueKind::String
    );
    assert_eq!(
        plan.after.get(PATH_PREPEND).unwrap().value,
        format!(r"C:\TOOLS\{A}\BIN")
    );
    assert_eq!(
        plan.after.get(PATH_PREPEND).unwrap().kind,
        ValueKind::String
    );
    assert_eq!(plan.after.get("PATH").unwrap().value, "%RELO_PATH_PREPEND%");
    assert_eq!(
        plan.after.get("PATH").unwrap().kind,
        ValueKind::ExpandString
    );
}

#[test]
fn path_uses_only_one_level_of_environment_expansion() {
    let plan = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    assert_eq!(
        plan.after.get("JAVA_HOME").unwrap().value,
        format!(r"C:\TOOLS\{A}")
    );
    assert_eq!(plan.after.get("PATH").unwrap().value, "%RELO_PATH_PREPEND%");
    assert!(!plan.after.get(PATH_PREPEND).unwrap().value.contains('%'));
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
fn aggregate_drift_is_rejected() {
    let first = plan_apply(Snapshot::default(), &desired(A), false).unwrap();
    let mut snapshot = first.after;
    snapshot.set(EnvValue::string(PATH_PREPEND, r"C:\manual"));
    let err = plan_apply(snapshot, &desired(B), false).unwrap_err();
    assert!(err.to_string().contains("does not match the context order"));
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
