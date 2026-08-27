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
fn cmd_path_expansion_diagnostic_probe() {
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
    let probe_name = "relo_path_expansion_probe.cmd";
    let probe_dir = std::env::temp_dir().join(format!("relo-path-expansion-{nonce}"));
    let probe_path = probe_dir.join(probe_name);
    let concrete_path = probe_dir.display().to_string();
    let system_root = std::env::var("SystemRoot").unwrap();
    let cmd = Path::new(&system_root).join("System32").join("cmd.exe");

    let result = (|| -> Result<String> {
        std::fs::create_dir_all(&probe_dir).context("create PATH expansion probe directory")?;
        std::fs::write(&probe_path, "@echo RELO_PATH_EXPANSION_OK\r\n")
            .context("write PATH expansion probe command")?;

        let direct = probe_environment(
            &system_root,
            &cmd,
            [
                ("PATH", provider_reference.as_str()),
                (provider_name.as_str(), concrete_path.as_str()),
            ],
        );
        let materialized = probe_environment(
            &system_root,
            &cmd,
            [
                ("PATH", aggregate_reference.as_str()),
                (PATH_APPEND, concrete_path.as_str()),
            ],
        );
        let nested = probe_environment(
            &system_root,
            &cmd,
            [
                ("PATH", aggregate_reference.as_str()),
                (PATH_APPEND, provider_reference.as_str()),
                (provider_name.as_str(), concrete_path.as_str()),
            ],
        );

        let mut registry = before.clone();
        registry.set(EnvValue::expandable(&provider_name, &concrete_path));
        registry.set(EnvValue::expandable(PATH_APPEND, &provider_reference));
        registry.set(EnvValue::expandable("Path", &aggregate_reference));
        apply(
            Scope::User,
            &Plan::between(before.clone(), registry, false, Vec::new()),
        )?;
        let fresh = fresh_user_environment()?;
        let fresh_probe = run_cmd_probe(&cmd, probe_name, &fresh)?;

        Ok([
            format!("provider name: {provider_name}"),
            format!("provider reference: {provider_reference}"),
            format!("aggregate reference: {aggregate_reference}"),
            format!("concrete path: {concrete_path}"),
            format!("probe command: {}", probe_path.display()),
            String::new(),
            "[direct: Path -> provider -> concrete]".to_owned(),
            direct?,
            String::new(),
            "[materialized: Path -> aggregate(concrete)]".to_owned(),
            materialized?,
            String::new(),
            "[nested: Path -> aggregate -> provider -> concrete]".to_owned(),
            nested?,
            String::new(),
            "[fresh user block from nested registry values]".to_owned(),
            format!(
                "provider={:?}\naggregate={:?}\nPath contains aggregate reference={}\n{}",
                fresh.get(&provider_name),
                fresh.get(PATH_APPEND),
                path_has_segment(&fresh, &aggregate_reference),
                fresh_probe
            ),
        ]
        .join("\n"))
    })();

    let registry_cleanup = read(Scope::User).and_then(|current| {
        apply(
            Scope::User,
            &Plan::between(current, before, false, Vec::new()),
        )
    });
    let file_cleanup = std::fs::remove_dir_all(&probe_dir);
    if let Err(error) = registry_cleanup {
        panic!("failed to restore Windows PATH expansion test environment: {error:#}");
    }
    match file_cleanup {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("failed to remove Windows PATH expansion probe directory: {error}");
        }
        _ => {}
    }
    match result {
        Ok(report) => panic!("CMD PATH expansion diagnostic completed:\n{report}"),
        Err(error) => panic!("CMD PATH expansion diagnostic failed before completion: {error:#}"),
    }
}

fn probe_environment<'a>(
    system_root: &str,
    cmd: &Path,
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<String> {
    let mut environment = BTreeMap::from([
        ("COMSPEC".to_owned(), cmd.display().to_string()),
        ("PATHEXT".to_owned(), ".COM;.EXE;.BAT;.CMD".to_owned()),
        ("SYSTEMROOT".to_owned(), system_root.to_owned()),
    ]);
    environment.extend(
        values
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned())),
    );
    run_cmd_probe(cmd, "relo_path_expansion_probe.cmd", &environment)
}

fn run_cmd_probe(
    cmd: &Path,
    probe_name: &str,
    environment: &BTreeMap<String, String>,
) -> Result<String> {
    let output = Command::new(cmd)
        .args(["/D", "/Q", "/C", probe_name])
        .env_clear()
        .envs(environment.iter().filter(|(name, _)| !name.is_empty()))
        .output()
        .context("start fresh cmd.exe for PATH expansion probe")?;
    Ok(format!(
        "Path={:?}\nstatus={:?}\nstdout={:?}\nstderr={:?}",
        environment.get("PATH"),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn path_has_segment(environment: &BTreeMap<String, String>, expected: &str) -> bool {
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
