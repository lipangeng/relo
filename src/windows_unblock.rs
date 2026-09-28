use anyhow::Result;
use std::path::Path;

pub fn ensure_supported() -> Result<()> {
    if cfg!(windows) {
        Ok(())
    } else {
        anyhow::bail!("win unblock is only supported on Windows")
    }
}

#[cfg(windows)]
pub fn unblock(release: &Path, verbose: bool) -> Result<()> {
    use std::fs;
    use std::os::windows::fs::MetadataExt;

    // Includes directory junctions and other reparse points, not only symlinks.
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let mut pending = vec![release.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|err| {
            anyhow::anyhow!("failed to inspect {}: {err}", crate::paths::display(&path))
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            if verbose {
                eprintln!("skipped reparse point: {}", crate::paths::display(&path));
            }
            continue;
        }
        if metadata.is_dir() {
            let entries = fs::read_dir(&path).map_err(|err| {
                anyhow::anyhow!(
                    "failed to read directory {}: {err}",
                    crate::paths::display(&path)
                )
            })?;
            for entry in entries {
                let entry = entry.map_err(|err| {
                    anyhow::anyhow!(
                        "failed to read entry in {}: {err}",
                        crate::paths::display(&path)
                    )
                })?;
                pending.push(entry.path());
            }
        } else if metadata.is_file() {
            // Append to the native path to preserve non-Unicode names and long paths.
            let mut stream = path.as_os_str().to_os_string();
            stream.push(":Zone.Identifier");
            match fs::remove_file(Path::new(&stream)) {
                Ok(()) => {
                    if verbose {
                        eprintln!("unblocked file: {}", crate::paths::display(&path));
                    }
                }
                // ERROR_FILE_NOT_FOUND: the file has no download mark (idempotent).
                Err(err) if err.raw_os_error() == Some(2) => {}
                Err(err) => anyhow::bail!(
                    "failed to remove Zone.Identifier from {}: {err}",
                    crate::paths::display(&path)
                ),
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn unblock(_release: &Path, _verbose: bool) -> Result<()> {
    unreachable!("platform support is checked before resolving the release")
}
