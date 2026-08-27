use anyhow::Result;
use std::path::Path;

#[cfg(target_os = "macos")]
pub fn ensure_supported() -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_supported() -> Result<()> {
    anyhow::bail!("mac unblock is only supported on macOS")
}

#[cfg(target_os = "macos")]
pub fn unblock(release: &Path, verbose: bool) -> Result<String> {
    use anyhow::Context;
    use std::process::Command;

    let verbose_output = if verbose {
        Command::new("/usr/bin/xattr")
            .args(["-p", "-r", "-s", "-v", "com.apple.quarantine"])
            .arg(release)
            .output()
            .with_context(|| "failed to run /usr/bin/xattr")?
            .stdout
    } else {
        Vec::new()
    };

    let output = Command::new("/usr/bin/xattr")
        .args(["-d", "-r", "-s"])
        .arg("com.apple.quarantine")
        .arg(release)
        .output()
        .with_context(|| "failed to run /usr/bin/xattr")?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if detail.is_empty() {
            anyhow::bail!(
                "failed to remove macOS quarantine attribute from {}",
                crate::paths::display(release)
            );
        }
        anyhow::bail!(
            "failed to remove macOS quarantine attribute from {}: {}",
            crate::paths::display(release),
            detail
        );
    }

    Ok(String::from_utf8_lossy(&verbose_output).into_owned())
}

#[cfg(not(target_os = "macos"))]
pub fn unblock(_release: &Path, _verbose: bool) -> Result<String> {
    unreachable!("platform support is checked before resolving the release")
}
