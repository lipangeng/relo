//! Keeps filesystem-native paths internal and removes Windows verbatim prefixes
//! only when a path crosses into CLI output, shell code, or environment values.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Formats a filesystem path for external consumption.
pub(crate) fn display(path: &Path) -> Cow<'_, str> {
    match external(path) {
        Cow::Borrowed(path) => path.to_string_lossy(),
        Cow::Owned(path) => Cow::Owned(path.to_string_lossy().into_owned()),
    }
}

/// Returns a path suitable for comparison with externally supplied paths.
pub(crate) fn external(path: &Path) -> Cow<'_, Path> {
    let value = path.to_string_lossy();
    let external = external_windows(&value);
    if external == value {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(PathBuf::from(external.as_ref()))
    }
}

/// Converts a Windows verbatim drive or UNC string into its ordinary form.
pub(crate) fn external_windows(value: &str) -> Cow<'_, str> {
    const VERBATIM_PREFIX: &str = r"\\?\";
    const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";

    if value
        .get(..VERBATIM_UNC_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(VERBATIM_UNC_PREFIX))
    {
        return Cow::Owned(format!(r"\\{}", &value[VERBATIM_UNC_PREFIX.len()..]));
    }
    if let Some(path) = value.strip_prefix(VERBATIM_PREFIX) {
        let bytes = path.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            return Cow::Borrowed(path);
        }
    }
    Cow::Borrowed(value)
}

#[cfg(test)]
mod tests {
    use super::{display, external_windows};
    use std::path::Path;

    #[test]
    fn display_hides_windows_verbatim_disk_prefix() {
        assert_eq!(
            display(Path::new(r"\\?\D:\20_Toolchain\Golang\releases\1.27.0")),
            r"D:\20_Toolchain\Golang\releases\1.27.0"
        );
    }

    #[test]
    fn external_windows_converts_verbatim_unc_prefix() {
        assert_eq!(
            external_windows(r"\\?\UNC\server\share\Relo\active\bin"),
            r"\\server\share\Relo\active\bin"
        );
    }

    #[test]
    fn external_windows_preserves_device_paths() {
        assert_eq!(
            external_windows(r"D:\Tools\Relo\active\bin"),
            r"D:\Tools\Relo\active\bin"
        );
        assert_eq!(
            external_windows(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Relo"),
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Relo"
        );
    }
}
