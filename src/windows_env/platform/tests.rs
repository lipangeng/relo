use super::*;
use crate::windows_env::model::{DesiredContext, PATH_APPEND};
use crate::windows_env::protocol::context_id_from_normalized;
use crate::windows_env::reconcile::{plan_apply, plan_remove};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[test]
fn persistent_environment_is_visible_in_a_fresh_user_environment_block() {
    if std::env::var_os("RELO_WINDOWS_ENV_INTEGRATION").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }

    let _guard = lock(Scope::User).unwrap();
    let before = read(Scope::User).unwrap();
    let nonce = std::process::id();
    let root = format!(r"C:\RELO-INTEGRATION-{nonce}");
    let id = context_id_from_normalized(&root);
    let public_name = format!("RELOIT_{nonce}");
    let concrete_value = format!("VALUE_{nonce}");
    let concrete_path = format!(r"{root}\BIN");
    let desired = DesiredContext {
        id: id.clone(),
        root,
        release: "1.0.0".to_owned(),
        path: concrete_path.clone(),
        env: BTreeMap::from([(public_name.clone(), concrete_value.clone())]),
    };

    let result = (|| -> Result<()> {
        let applied = plan_apply(before.clone(), &desired, false)?;
        apply(Scope::User, &applied)?;
        let persisted = read(Scope::User)?;
        let repeated = plan_apply(persisted.clone(), &desired, false)?;
        if !repeated.is_empty() {
            bail!("repeated apply was not idempotent");
        }

        let environment = fresh_user_environment()?;
        if environment.get(&public_name.to_ascii_uppercase()) != Some(&concrete_value) {
            bail!("provider reference was not expanded in a fresh environment block");
        }
        let effective_path = environment.get("PATH").cloned().unwrap_or_default();
        if !effective_path
            .split(';')
            .any(|entry| entry.eq_ignore_ascii_case(&concrete_path))
        {
            bail!("nested relo PATH references were not expanded");
        }

        let removed = plan_remove(persisted, &id)?;
        apply(Scope::User, &removed)?;
        Ok(())
    })();

    let cleanup = read(Scope::User).and_then(|current| {
        let restore = Plan::between(current, before, false, Vec::new());
        apply(Scope::User, &restore)
    });
    if let Err(error) = cleanup {
        panic!("failed to restore Windows integration-test environment: {error:#}");
    }
    result.unwrap();
}

#[test]
fn ordered_nested_path_expansion_diagnostic_probe() {
    if std::env::var_os("RELO_WINDOWS_ENV_INTEGRATION").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }

    let _guard = lock(Scope::User).unwrap();
    let before = read(Scope::User).unwrap();
    let nonce = std::process::id();
    let provider_name = format!("RELO_PATH_EXPANSION_TEST_{nonce}");
    let provider_reference = format!("%{provider_name}%");
    let aggregate_reference = format!("%{PATH_APPEND}%");
    let probe_name = "relo_ordered_path_probe.cmd";
    let probe_dir = std::env::temp_dir().join(format!("relo-ordered-path-{nonce}"));
    let probe_path = probe_dir.join(probe_name);
    let concrete_path = probe_dir.display().to_string();
    let system_root = std::env::var("SystemRoot").unwrap();
    let cmd = Path::new(&system_root).join("System32").join("cmd.exe");

    let result = (|| -> Result<String> {
        std::fs::create_dir_all(&probe_dir).context("create ordered PATH probe directory")?;
        std::fs::write(&probe_path, "@echo RELO_ORDERED_PATH_OK\r\n")
            .context("write ordered PATH probe command")?;

        let mut cleared = before.clone();
        cleared.remove("Path");
        cleared.remove(PATH_APPEND);
        cleared.remove(&provider_name);
        apply(
            Scope::User,
            &Plan::between(before.clone(), cleared.clone(), false, Vec::new()),
        )?;

        let mut provider_stage = cleared;
        provider_stage.set(EnvValue::expandable(&provider_name, &concrete_path));
        apply(
            Scope::User,
            &Plan::between(read(Scope::User)?, provider_stage, false, Vec::new()),
        )?;

        let mut aggregate_stage = read(Scope::User)?;
        aggregate_stage.set(EnvValue::expandable(PATH_APPEND, &provider_reference));
        apply(
            Scope::User,
            &Plan::between(read(Scope::User)?, aggregate_stage, false, Vec::new()),
        )?;

        let mut path_stage = read(Scope::User)?;
        path_stage.set(EnvValue::expandable("Path", &aggregate_reference));
        apply(
            Scope::User,
            &Plan::between(read(Scope::User)?, path_stage, false, Vec::new()),
        )?;

        let registry = read(Scope::User)?;
        let fresh = fresh_user_environment()?;
        let command = run_ordered_path_probe(&cmd, probe_name, &fresh)?;
        let fresh_path = fresh.get("PATH").cloned().unwrap_or_default();
        let fresh_path_tail = fresh_path
            .split(';')
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(";");

        Ok(format!(
            concat!(
                "write order: provider -> aggregate -> Path\n",
                "provider name: {provider_name}\n",
                "provider reference: {provider_reference}\n",
                "aggregate reference: {aggregate_reference}\n",
                "concrete path: {concrete_path}\n",
                "registry provider: {registry_provider}\n",
                "registry aggregate: {registry_aggregate}\n",
                "registry Path: {registry_path}\n",
                "fresh provider: {fresh_provider}\n",
                "fresh aggregate: {fresh_aggregate}\n",
                "fresh Path contains aggregate reference: {has_aggregate_reference}\n",
                "fresh Path contains provider reference: {has_provider_reference}\n",
                "fresh Path contains concrete path: {has_concrete_path}\n",
                "fresh Path tail: {fresh_path_tail:?}\n",
                "{command}"
            ),
            provider_name = provider_name,
            provider_reference = provider_reference,
            aggregate_reference = aggregate_reference,
            concrete_path = concrete_path,
            registry_provider = diagnostic_registry_value(&registry, &provider_name),
            registry_aggregate = diagnostic_registry_value(&registry, PATH_APPEND),
            registry_path = diagnostic_registry_value(&registry, "Path"),
            fresh_provider = diagnostic_environment_value(&fresh, &provider_name),
            fresh_aggregate = diagnostic_environment_value(&fresh, PATH_APPEND),
            has_aggregate_reference = diagnostic_path_has(&fresh, &aggregate_reference),
            has_provider_reference = diagnostic_path_has(&fresh, &provider_reference),
            has_concrete_path = diagnostic_path_has(&fresh, &concrete_path),
            fresh_path_tail = fresh_path_tail,
            command = command,
        ))
    })();

    let registry_cleanup = read(Scope::User).and_then(|current| {
        apply(
            Scope::User,
            &Plan::between(current, before, false, Vec::new()),
        )
    });
    let file_cleanup = std::fs::remove_dir_all(&probe_dir);
    if let Err(error) = registry_cleanup {
        panic!("failed to restore ordered PATH probe environment: {error:#}");
    }
    match file_cleanup {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("failed to remove ordered PATH probe directory: {error}");
        }
        _ => {}
    }
    match result {
        Ok(report) => panic!("Ordered PATH expansion diagnostic completed:\n{report}"),
        Err(error) => {
            panic!("Ordered PATH expansion diagnostic failed before completion: {error:#}")
        }
    }
}

fn run_ordered_path_probe(
    cmd: &Path,
    probe_name: &str,
    environment: &BTreeMap<String, String>,
) -> Result<String> {
    let output = Command::new(cmd)
        .args(["/D", "/Q", "/C", probe_name])
        .env_clear()
        .envs(environment.iter().filter(|(name, _)| !name.is_empty()))
        .output()
        .context("start cmd.exe for ordered PATH probe")?;
    Ok(format!(
        "cmd status: {:?}\ncmd stdout: {:?}\ncmd stderr: {:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn diagnostic_registry_value(snapshot: &Snapshot, name: &str) -> String {
    snapshot.get(name).map_or_else(
        || "<missing>".to_owned(),
        |value| format!("kind={:?}, value={:?}", value.kind, value.value),
    )
}

fn diagnostic_environment_value(environment: &BTreeMap<String, String>, name: &str) -> String {
    environment
        .get(&name.to_ascii_uppercase())
        .map_or_else(|| "<missing>".to_owned(), |value| format!("{value:?}"))
}

fn diagnostic_path_has(environment: &BTreeMap<String, String>, expected: &str) -> bool {
    environment.get("PATH").is_some_and(|path| {
        path.split(';')
            .any(|segment| segment.eq_ignore_ascii_case(expected))
    })
}

fn fresh_user_environment() -> Result<BTreeMap<String, String>> {
    let mut token = null_mut();
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    };
    if opened == 0 {
        return Err(std::io::Error::last_os_error()).context("open current process token");
    }
    let mut block = null_mut();
    let created = unsafe { CreateEnvironmentBlock(&mut block, token, 0) };
    unsafe { CloseHandle(token) };
    if created == 0 {
        return Err(std::io::Error::last_os_error()).context("create user environment block");
    }

    let result = unsafe { parse_environment_block(block.cast::<u16>()) };
    unsafe { DestroyEnvironmentBlock(block) };
    result
}

unsafe fn parse_environment_block(mut cursor: *const u16) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    loop {
        let mut len = 0usize;
        while unsafe { *cursor.add(len) } != 0 {
            len += 1;
            if len > 1_048_576 {
                bail!("unterminated Windows environment block");
            }
        }
        if len == 0 {
            break;
        }
        let entry = String::from_utf16(unsafe { std::slice::from_raw_parts(cursor, len) })?;
        if let Some((name, value)) = entry.split_once('=') {
            values.insert(name.to_ascii_uppercase(), value.to_owned());
        }
        cursor = unsafe { cursor.add(len + 1) };
    }
    Ok(values)
}
