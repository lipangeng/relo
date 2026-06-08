use crate::layout::Layout;
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    Powershell,
    Cmd,
}

impl ShellKind {
    pub fn default_for_platform() -> Self {
        if cfg!(windows) {
            ShellKind::Powershell
        } else {
            ShellKind::Posix
        }
    }
}

pub fn exports(
    layout: &Layout,
    release_id: &str,
    shell: ShellKind,
    path: &[String],
    append_path: bool,
) -> Result<String> {
    match shell {
        ShellKind::Posix => posix_exports(layout, release_id, path, append_path),
        ShellKind::Powershell => powershell_exports(layout, release_id, path, append_path),
        ShellKind::Cmd => cmd_exports(layout, release_id, path, append_path),
    }
}

pub fn forward<S: AsRef<str>>(args: &[S], shell: ShellKind) -> Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.display().to_string();
    let args = args.iter().map(|arg| arg.as_ref()).collect::<Vec<_>>();
    let out = match shell {
        ShellKind::Posix => format!(
            "{} {}\n",
            quote_posix(&exe),
            args.iter()
                .map(|arg| quote_posix(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ShellKind::Powershell => format!(
            "& {} {}\n",
            quote_powershell(&exe),
            args.iter()
                .map(|arg| quote_powershell(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        ShellKind::Cmd => format!(
            "{} {}\n",
            quote_cmd(&exe),
            args.iter()
                .map(|arg| quote_cmd(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    };
    Ok(out)
}

fn posix_exports(
    layout: &Layout,
    release_id: &str,
    path: &[String],
    append_path: bool,
) -> Result<String> {
    let mut out = String::new();
    for (name, value) in layout.effective_env(release_id)? {
        out.push_str(&format!("export {name}=\"{}\"\n", escape_posix(&value)));
    }

    let path = layout.effective_path(release_id, path)?;
    if !path.is_empty() {
        let paths = path
            .iter()
            .map(|path| escape_posix(&path.display().to_string()))
            .collect::<Vec<_>>()
            .join(":");
        if append_path {
            out.push_str(&format!("export PATH=\"$PATH:{}\"\n", paths));
        } else {
            out.push_str(&format!("export PATH=\"{}:$PATH\"\n", paths));
        }
    }

    Ok(out)
}

fn powershell_exports(
    layout: &Layout,
    release_id: &str,
    path: &[String],
    append_path: bool,
) -> Result<String> {
    let mut out = String::new();
    for (name, value) in layout.effective_env(release_id)? {
        out.push_str(&format!("$env:{name} = '{}'\n", escape_powershell(&value)));
    }

    let path = layout.effective_path(release_id, path)?;
    if !path.is_empty() {
        let paths = path
            .iter()
            .map(|path| format!("'{}'", escape_powershell(&path.display().to_string())))
            .collect::<Vec<_>>()
            .join(" + ';' + ");
        if append_path {
            out.push_str(&format!("$env:PATH = $env:PATH + ';' + {}\n", paths));
        } else {
            out.push_str(&format!("$env:PATH = {} + ';' + $env:PATH\n", paths));
        }
    }

    Ok(out)
}

fn cmd_exports(
    layout: &Layout,
    release_id: &str,
    path: &[String],
    append_path: bool,
) -> Result<String> {
    let mut out = String::new();
    for (name, value) in layout.effective_env(release_id)? {
        out.push_str(&format!("set \"{name}={}\"\n", escape_cmd(&value)));
    }

    let path = layout.effective_path(release_id, path)?;
    if !path.is_empty() {
        let paths = path
            .iter()
            .map(|path| escape_cmd(&path.display().to_string()))
            .collect::<Vec<_>>()
            .join(";");
        if append_path {
            out.push_str(&format!("set \"PATH=%PATH%;{}\"\n", paths));
        } else {
            out.push_str(&format!("set \"PATH={};%PATH%\"\n", paths));
        }
    }

    Ok(out)
}

pub fn wrapper(shell: ShellKind) -> &'static str {
    match shell {
        ShellKind::Posix => posix_wrapper(),
        ShellKind::Powershell => powershell_wrapper(),
        ShellKind::Cmd => cmd_wrapper(),
    }
}

fn posix_wrapper() -> &'static str {
    r#"relo() {
  if [ "$1" = "use" ]; then
    shift
    eval "$(command relo use --shell posix "$@")"
  else
    command relo "$@"
  fi
}
"#
}

fn powershell_wrapper() -> &'static str {
    r#"function relo {
  if ($args.Count -gt 0 -and $args[0] -eq "use") {
    $rest = @()
    if ($args.Count -gt 1) {
      $rest = $args[1..($args.Count - 1)]
    }
    Invoke-Expression (& relo.exe use --shell powershell @rest)
  } else {
    & relo.exe @args
  }
}
"#
}

fn cmd_wrapper() -> &'static str {
    r#"doskey relo=if "$1" == "use" (for /f "delims=" %i in ('relo.exe use --shell cmd $2 $3 $4 $5 $6 $7 $8 $9') do %i) else relo.exe $*
"#
}

fn escape_posix(value: &str) -> String {
    // The generated script is evaluated by the user's shell, so escape only
    // the characters that can break a double-quoted export value.
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

fn quote_posix(value: &str) -> String {
    format!("\"{}\"", escape_posix(value))
}

fn escape_powershell(value: &str) -> String {
    value.replace('\'', "''")
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", escape_powershell(value))
}

fn escape_cmd(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('"', "^\"")
}

fn quote_cmd(value: &str) -> String {
    format!("\"{}\"", escape_cmd(value))
}
