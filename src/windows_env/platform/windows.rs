use super::model::{Change, EnvValue, Plan, Scope, Snapshot, ValueKind};
use super::protocol::context_id_from_normalized;
use anyhow::{bail, Context, Result};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ,
    REG_SZ,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

const LOCK_TIMEOUT_MS: u32 = 30_000;
const BROADCAST_TIMEOUT_MS: u32 = 5_000;

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

pub(super) struct LockGuard(HANDLE);

impl Drop for LockGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub(super) fn read(scope: Scope) -> Result<Snapshot> {
    let key = open_key(scope, KEY_QUERY_VALUE)?;
    let (count, max_name, max_data) = query_sizes(&key)?;
    let mut values = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut name = vec![0u16; max_name as usize + 2];
        let mut data = vec![0u8; max_data as usize + 2];
        let mut name_len = name.len() as u32;
        let mut data_len = data.len() as u32;
        let mut value_type = 0u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                null(),
                &mut value_type,
                data.as_mut_ptr(),
                &mut data_len,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        check(status, "enumerate environment variables")?;
        if value_type != REG_SZ && value_type != REG_EXPAND_SZ {
            bail!(
                "unsupported registry type {value_type} for environment variable {}",
                String::from_utf16_lossy(&name[..name_len as usize])
            );
        }
        if !data_len.is_multiple_of(2) {
            bail!("environment registry contains invalid UTF-16 data");
        }
        let words = unsafe {
            std::slice::from_raw_parts(data.as_ptr().cast::<u16>(), data_len as usize / 2)
        };
        let value_len = words
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(words.len());
        let name = String::from_utf16(&name[..name_len as usize])
            .context("environment variable name is not valid UTF-16")?;
        let value = String::from_utf16(&words[..value_len])
            .with_context(|| format!("environment variable {name} is not valid UTF-16"))?;
        values.push(EnvValue {
            name,
            value,
            kind: if value_type == REG_EXPAND_SZ {
                ValueKind::ExpandString
            } else {
                ValueKind::String
            },
        });
    }
    Snapshot::from_values(values)
}

pub(super) fn lock(scope: Scope) -> Result<LockGuard> {
    let name = match scope {
        Scope::User => {
            let domain = std::env::var("USERDOMAIN").unwrap_or_default();
            let user = std::env::var("USERNAME").unwrap_or_default();
            format!(
                r"Global\relo-env-user-{}",
                context_id_from_normalized(&format!(r"{domain}\{user}"))
            )
        }
        Scope::System => r"Global\relo-env-system".to_owned(),
    };
    let name = wide(&name);
    let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error()).context("create Windows environment mutex");
    }
    match unsafe { WaitForSingleObject(handle, LOCK_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(LockGuard(handle)),
        WAIT_TIMEOUT => {
            unsafe { CloseHandle(handle) };
            bail!(
                "timed out waiting for the {} environment lock",
                scope.as_str()
            )
        }
        _ => {
            unsafe { CloseHandle(handle) };
            Err(std::io::Error::last_os_error()).context("wait for Windows environment mutex")
        }
    }
}

pub(super) fn apply(scope: Scope, plan: &Plan) -> Result<()> {
    let key = open_key(scope, KEY_SET_VALUE)?;
    let mut applied = Vec::new();
    for change in &plan.changes {
        let result = write_change(&key, change, false);
        if let Err(error) = result {
            let rollback = rollback(&key, &applied);
            return match rollback {
                Ok(()) => Err(error).context("apply persistent environment; changes rolled back"),
                Err(rollback_error) => Err(error).context(format!(
                    "apply persistent environment; rollback also failed: {rollback_error:#}"
                )),
            };
        }
        applied.push(change);
    }
    Ok(())
}

pub(super) fn broadcast() -> Result<()> {
    let environment = wide("Environment");
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            BROADCAST_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 {
        return Err(std::io::Error::last_os_error()).context("broadcast WM_SETTINGCHANGE");
    }
    Ok(())
}

fn rollback(key: &RegistryKey, applied: &[&Change]) -> Result<()> {
    let mut failures = Vec::new();
    for change in applied.iter().rev() {
        if let Err(error) = write_change(key, change, true) {
            failures.push(format!("{}: {error:#}", change.name));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("{}", failures.join("; "))
    }
}

fn write_change(key: &RegistryKey, change: &Change, rollback: bool) -> Result<()> {
    let value = if rollback {
        &change.before
    } else {
        &change.after
    };
    match value {
        Some(value) => set_value(key, value),
        None => delete_value(key, &change.name),
    }
}

fn open_key(scope: Scope, access: u32) -> Result<RegistryKey> {
    let (root, subkey) = match scope {
        Scope::User => (HKEY_CURRENT_USER, "Environment"),
        Scope::System => (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    };
    let subkey = wide(subkey);
    let mut key = null_mut();
    let status = unsafe { RegOpenKeyExW(root, subkey.as_ptr(), 0, access, &mut key) };
    check(status, "open Windows environment registry key").with_context(|| {
        if scope == Scope::System {
            "system scope requires an administrator terminal"
        } else {
            "cannot access user environment"
        }
    })?;
    Ok(RegistryKey(key))
}

fn query_sizes(key: &RegistryKey) -> Result<(u32, u32, u32)> {
    let mut count = 0u32;
    let mut max_name = 0u32;
    let mut max_data = 0u32;
    let status = unsafe {
        RegQueryInfoKeyW(
            key.0,
            null_mut(),
            null_mut(),
            null(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut count,
            &mut max_name,
            &mut max_data,
            null_mut(),
            null_mut(),
        )
    };
    check(status, "query Windows environment registry")?;
    Ok((count, max_name, max_data))
}

fn set_value(key: &RegistryKey, value: &EnvValue) -> Result<()> {
    let name = wide(&value.name);
    let data = wide(&value.value);
    let value_type = match value.kind {
        ValueKind::String => REG_SZ,
        ValueKind::ExpandString => REG_EXPAND_SZ,
    };
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            value_type,
            data.as_ptr().cast::<u8>(),
            (data.len() * size_of::<u16>()) as u32,
        )
    };
    check(
        status,
        &format!("write environment variable {}", value.name),
    )
}

fn delete_value(key: &RegistryKey, name: &str) -> Result<()> {
    let name = wide(name);
    let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
    check(status, "delete environment variable")
}

fn check(status: u32, action: &str) -> Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32)).context(action.to_owned())
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(all(test, windows))]
#[path = "tests.rs"]
mod tests;
