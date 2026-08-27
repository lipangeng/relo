use crate::cli::{EnvScopeArg, WinEnvCommand};
use crate::layout::Layout;
use anyhow::{bail, Context, Result};
use std::io::{self, IsTerminal, Write};
use std::path::Path;

mod model;
mod protocol;
mod reconcile;
mod status;
use model::{Plan, Scope, Snapshot, CONTEXT_PREFIX};
use protocol::{
    context_id, is_under_user_profile, normalize_context_path, validate_context_id, windows_path_eq,
};
use reconcile::{desired_context, plan_apply, plan_prune, plan_remove};
use status::{build_status, print_status, scope_shadow_warnings};

#[cfg(windows)]
pub fn ensure_supported() -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_supported() -> Result<()> {
    bail!("win env is only supported on Windows")
}

impl From<EnvScopeArg> for Scope {
    fn from(value: EnvScopeArg) -> Self {
        match value {
            EnvScopeArg::User => Self::User,
            EnvScopeArg::System => Self::System,
        }
    }
}

pub fn run(logical_root: &Path, resolved_root: &Path, command: WinEnvCommand) -> Result<()> {
    match command {
        WinEnvCommand::Apply {
            version,
            scope,
            path_append,
            yes,
            dry_run,
        } => {
            let layout = Layout::load(resolved_root.to_path_buf())?;
            let release = match version {
                Some(expr) => layout.resolve(&expr)?,
                None => {
                    let active = layout
                        .active_version()?
                        .context("no active release; specify a release or run `relo use -g`")?;
                    layout.resolve(&active)?
                }
            };
            let desired = desired_context(&layout, logical_root, &release.id)?;
            let scope = Scope::from(scope);
            execute_write(scope, yes, dry_run, |snapshot| {
                let mut plan = plan_apply(snapshot, &desired, path_append)?;
                if scope == Scope::System && is_under_user_profile(&desired.root) {
                    plan.notes.push(format!(
                        "system scope context is inside the current user profile: {}",
                        desired.root
                    ));
                }
                Ok(plan)
            })
        }
        WinEnvCommand::Remove {
            scope,
            id,
            yes,
            dry_run,
        } => {
            let (id, expected_path) = match id {
                Some(id) => {
                    validate_context_id(&id)?;
                    (id, None)
                }
                None => (
                    context_id(logical_root)?,
                    Some(normalize_context_path(logical_root)?),
                ),
            };
            execute_write(scope.into(), yes, dry_run, |snapshot| {
                if let (Some(expected), Some(binding)) = (
                    expected_path.as_deref(),
                    snapshot.get(&format!("{CONTEXT_PREFIX}{id}")),
                ) {
                    if !windows_path_eq(expected, &binding.value) {
                        bail!(
                            "context id collision: {id} is bound to {} instead of {expected}",
                            binding.value
                        );
                    }
                }
                plan_remove(snapshot, &id)
            })
        }
        WinEnvCommand::Prune {
            scope,
            yes,
            dry_run,
        } => execute_write(scope.into(), yes, dry_run, plan_prune),
        WinEnvCommand::Status { scope, all, json } => {
            let scope = scope.into();
            let snapshot = platform::read(scope)?;
            let current = (!all).then(|| context_id(logical_root)).transpose()?;
            let mut report = build_status(&snapshot, scope, current.as_deref());
            if scope == Scope::System {
                if let Ok(user) = platform::read(Scope::User) {
                    report
                        .warnings
                        .extend(scope_shadow_warnings(&snapshot, &user));
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_status(&report);
            }
            if !report.healthy {
                bail!("persistent Windows environment is not healthy");
            }
            Ok(())
        }
    }
}

fn execute_write<F>(scope: Scope, yes: bool, dry_run: bool, build: F) -> Result<()>
where
    F: FnOnce(Snapshot) -> Result<Plan>,
{
    if dry_run {
        let snapshot = platform::read(scope)?;
        let plan = build(snapshot)?;
        print_plan(&plan, scope, true);
        return Ok(());
    }

    let _guard = platform::lock(scope)?;
    let snapshot = platform::read(scope)?;
    let plan = build(snapshot)?;
    print_plan(&plan, scope, false);
    if plan.is_empty() {
        return Ok(());
    }
    if plan.requires_confirmation && !yes {
        confirm()?;
    }
    platform::apply(scope, &plan)?;
    if let Err(err) = platform::broadcast() {
        bail!(
            "environment was persisted, but Windows notification failed: {err}; sign out or restart Explorer"
        );
    }
    Ok(())
}

fn print_plan(plan: &Plan, scope: Scope, dry_run: bool) {
    println!("scope: {}", scope.as_str());
    println!("mode:  {}", if dry_run { "dry-run" } else { "apply" });
    for note in &plan.notes {
        println!("warning: {note}");
    }
    if plan.changes.is_empty() {
        println!("changes: none");
        return;
    }
    println!("changes:");
    for change in &plan.changes {
        match (&change.before, &change.after) {
            (None, Some(after)) => println!("  {}: <missing> -> {}", change.name, after.value),
            (Some(before), None) => println!("  {}: {} -> <deleted>", change.name, before.value),
            (Some(before), Some(after)) => {
                println!("  {}: {} -> {}", change.name, before.value, after.value)
            }
            (None, None) => unreachable!(),
        }
    }
}

fn confirm() -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("confirmation required; rerun with --yes or inspect with --dry-run");
    }
    print!("Continue? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("cancelled");
    }
    Ok(())
}

#[cfg(not(windows))]
#[path = "platform/unsupported.rs"]
mod platform;

#[cfg(windows)]
#[path = "platform/windows.rs"]
mod platform;

// Type-check the Win32 adapter on non-Windows development hosts without
// linking or executing it. Real behavior is still covered on Windows CI.
#[cfg(all(not(windows), feature = "check-windows"))]
#[allow(dead_code)]
#[path = "platform/windows.rs"]
mod platform_check;

#[cfg(test)]
mod tests;
