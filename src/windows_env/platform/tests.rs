use super::*;
use crate::windows_env::model::{DesiredContext, PATH_APPEND};
use crate::windows_env::protocol::context_id_from_normalized;
use crate::windows_env::reconcile::{plan_apply, plan_remove};
use std::collections::BTreeMap;
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
fn path_expansion_diagnostic_probe() {
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
    let concrete_path = format!(r"C:\RELO-PATH-EXPANSION-{nonce}\BIN");

    let result = (|| -> Result<String> {
        let mut one_level = before.clone();
        one_level.set(EnvValue::expandable(&provider_name, &concrete_path));
        one_level.set(EnvValue::expandable("Path", &provider_reference));
        apply(
            Scope::User,
            &Plan::between(before.clone(), one_level.clone(), false, Vec::new()),
        )?;
        let one_level_registry = read(Scope::User)?;
        let one_level_environment = fresh_user_environment()?;

        let mut two_levels = one_level.clone();
        two_levels.set(EnvValue::expandable(PATH_APPEND, &provider_reference));
        two_levels.set(EnvValue::expandable("Path", format!("%{PATH_APPEND}%")));
        apply(
            Scope::User,
            &Plan::between(one_level, two_levels, false, Vec::new()),
        )?;
        let two_level_registry = read(Scope::User)?;
        let two_level_environment = fresh_user_environment()?;

        Ok(format!(
            concat!(
                "provider name: {provider_name}\n",
                "provider reference: {provider_reference}\n",
                "aggregate name: {aggregate_name}\n",
                "aggregate reference: {aggregate_reference}\n",
                "concrete path: {concrete_path}\n",
                "\n[one level: Path -> provider]\n",
                "registry provider: {one_registry_provider}\n",
                "registry Path: {one_registry_path}\n",
                "fresh provider: {one_fresh_provider}\n",
                "fresh Path: {one_fresh_path}\n",
                "fresh Path contains provider reference: {one_has_provider_reference}\n",
                "fresh Path contains concrete path: {one_has_concrete_path}\n",
                "\n[two levels: Path -> aggregate -> provider]\n",
                "registry provider: {two_registry_provider}\n",
                "registry aggregate: {two_registry_aggregate}\n",
                "registry Path: {two_registry_path}\n",
                "fresh provider: {two_fresh_provider}\n",
                "fresh aggregate: {two_fresh_aggregate}\n",
                "fresh Path: {two_fresh_path}\n",
                "fresh Path contains aggregate reference: {two_has_aggregate_reference}\n",
                "fresh Path contains provider reference: {two_has_provider_reference}\n",
                "fresh Path contains concrete path: {two_has_concrete_path}"
            ),
            aggregate_name = PATH_APPEND,
            aggregate_reference = format!("%{PATH_APPEND}%"),
            one_registry_provider = registry_value(&one_level_registry, &provider_name),
            one_registry_path = registry_value(&one_level_registry, "Path"),
            one_fresh_provider = environment_value(&one_level_environment, &provider_name),
            one_fresh_path = environment_value(&one_level_environment, "Path"),
            one_has_provider_reference = path_contains(&one_level_environment, &provider_reference),
            one_has_concrete_path = path_contains(&one_level_environment, &concrete_path),
            two_registry_provider = registry_value(&two_level_registry, &provider_name),
            two_registry_aggregate = registry_value(&two_level_registry, PATH_APPEND),
            two_registry_path = registry_value(&two_level_registry, "Path"),
            two_fresh_provider = environment_value(&two_level_environment, &provider_name),
            two_fresh_aggregate = environment_value(&two_level_environment, PATH_APPEND),
            two_fresh_path = environment_value(&two_level_environment, "Path"),
            two_has_aggregate_reference =
                path_contains(&two_level_environment, &format!("%{PATH_APPEND}%")),
            two_has_provider_reference = path_contains(&two_level_environment, &provider_reference),
            two_has_concrete_path = path_contains(&two_level_environment, &concrete_path),
        ))
    })();

    let cleanup = read(Scope::User).and_then(|current| {
        let restore = Plan::between(current, before, false, Vec::new());
        apply(Scope::User, &restore)
    });
    if let Err(error) = cleanup {
        panic!("failed to restore Windows PATH expansion test environment: {error:#}");
    }
    match result {
        Ok(report) => panic!("PATH expansion diagnostic completed:\n{report}"),
        Err(error) => panic!("PATH expansion diagnostic failed before completion: {error:#}"),
    }
}

fn path_contains(environment: &BTreeMap<String, String>, expected: &str) -> bool {
    environment.get("PATH").is_some_and(|effective_path| {
        effective_path
            .split(';')
            .any(|entry| entry.eq_ignore_ascii_case(expected))
    })
}

fn registry_value(snapshot: &Snapshot, name: &str) -> String {
    snapshot.get(name).map_or_else(
        || "<missing>".to_owned(),
        |value| format!("kind={:?}, value={:?}", value.kind, value.value),
    )
}

fn environment_value(environment: &BTreeMap<String, String>, name: &str) -> String {
    environment
        .get(&name.to_ascii_uppercase())
        .map_or_else(|| "<missing>".to_owned(), |value| format!("{value:?}"))
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
