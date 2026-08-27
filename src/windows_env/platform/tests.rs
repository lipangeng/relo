use super::*;
use crate::windows_env::model::{DesiredContext, CONF_PATH_PREPEND, PATH_PREPEND};
use crate::windows_env::protocol::{context_id_from_normalized, reference};
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
        if persisted.get(CONF_PATH_PREPEND).map(|value| &value.value) != Some(&id) {
            bail!("PATH context order was not persisted");
        }
        if persisted.get(PATH_PREPEND).map(|value| &value.value) != Some(&concrete_path) {
            bail!("PATH aggregate was not materialized");
        }
        let persisted_path = persisted
            .get("Path")
            .map(|value| value.value.as_str())
            .unwrap_or_default();
        let prepend_anchor = reference(PATH_PREPEND);
        if !persisted_path
            .split(';')
            .next()
            .is_some_and(|segment| segment.eq_ignore_ascii_case(&prepend_anchor))
        {
            bail!("Path does not contain the prepend aggregate anchor");
        }
        let repeated = plan_apply(persisted.clone(), &desired, false)?;
        if !repeated.is_empty() {
            bail!("repeated apply was not idempotent");
        }

        let environment = fresh_user_environment()?;
        if environment.get(&public_name.to_ascii_uppercase()) != Some(&concrete_value) {
            bail!("public environment value was not visible in a fresh environment block");
        }
        if environment.get(PATH_PREPEND) != Some(&concrete_path) {
            bail!("materialized PATH aggregate was not visible in a fresh environment block");
        }
        let effective_path = environment.get("PATH").cloned().unwrap_or_default();
        if !effective_path
            .split(';')
            .any(|entry| entry.eq_ignore_ascii_case(&concrete_path))
        {
            bail!("the one-level relo PATH aggregate was not expanded");
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
