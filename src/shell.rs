use crate::layout::Layout;
use anyhow::Result;
use std::path::Path;

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

pub fn exports(layout: &Layout, release_id: &str, shell: ShellKind) -> Result<String> {
    match shell {
        ShellKind::Posix => posix_exports(layout, release_id),
        ShellKind::Powershell => powershell_exports(layout, release_id),
        ShellKind::Cmd => cmd_exports(layout, release_id),
    }
}

fn posix_exports(layout: &Layout, release_id: &str) -> Result<String> {
    let release = layout.release_path(release_id);
    let home = layout.home_for(release_id);
    let mut out = String::new();
    out.push_str(&format!(
        "export RELO_ROOT=\"{}\"\n",
        escape_posix(&layout.root.display().to_string())
    ));
    out.push_str(&format!(
        "export RELO_RELEASE=\"{}\"\n",
        escape_posix(&release.display().to_string())
    ));
    out.push_str(&format!(
        "export RELO_HOME=\"{}\"\n",
        escape_posix(&home.display().to_string())
    ));

    for (name, value) in &layout.config.env {
        let path = layout.env_path(value, release_id);
        out.push_str(&format!(
            "export {name}=\"{}\"\n",
            escape_posix(&path.display().to_string())
        ));
    }

    for bin in &layout.config.bin {
        let bin_path = bin_path(layout, &release, bin);
        out.push_str(&format!(
            "export PATH=\"{}:$PATH\"\n",
            escape_posix(&bin_path.display().to_string())
        ));
    }

    Ok(out)
}

fn powershell_exports(layout: &Layout, release_id: &str) -> Result<String> {
    let release = layout.release_path(release_id);
    let home = layout.home_for(release_id);
    let mut out = String::new();
    out.push_str(&format!(
        "$env:RELO_ROOT = '{}'\n",
        escape_powershell(&layout.root.display().to_string())
    ));
    out.push_str(&format!(
        "$env:RELO_RELEASE = '{}'\n",
        escape_powershell(&release.display().to_string())
    ));
    out.push_str(&format!(
        "$env:RELO_HOME = '{}'\n",
        escape_powershell(&home.display().to_string())
    ));

    for (name, value) in &layout.config.env {
        let path = layout.env_path(value, release_id);
        out.push_str(&format!(
            "$env:{name} = '{}'\n",
            escape_powershell(&path.display().to_string())
        ));
    }

    for bin in &layout.config.bin {
        let bin_path = bin_path(layout, &release, bin);
        out.push_str(&format!(
            "$env:PATH = '{}' + ';' + $env:PATH\n",
            escape_powershell(&bin_path.display().to_string())
        ));
    }

    Ok(out)
}

fn cmd_exports(layout: &Layout, release_id: &str) -> Result<String> {
    let release = layout.release_path(release_id);
    let home = layout.home_for(release_id);
    let mut out = String::new();
    out.push_str(&format!(
        "set \"RELO_ROOT={}\"\n",
        escape_cmd(&layout.root.display().to_string())
    ));
    out.push_str(&format!(
        "set \"RELO_RELEASE={}\"\n",
        escape_cmd(&release.display().to_string())
    ));
    out.push_str(&format!(
        "set \"RELO_HOME={}\"\n",
        escape_cmd(&home.display().to_string())
    ));

    for (name, value) in &layout.config.env {
        let path = layout.env_path(value, release_id);
        out.push_str(&format!(
            "set \"{name}={}\"\n",
            escape_cmd(&path.display().to_string())
        ));
    }

    for bin in &layout.config.bin {
        let bin_path = bin_path(layout, &release, bin);
        out.push_str(&format!(
            "set \"PATH={};%PATH%\"\n",
            escape_cmd(&bin_path.display().to_string())
        ));
    }

    Ok(out)
}

fn bin_path(layout: &Layout, release: &Path, bin: &str) -> std::path::PathBuf {
    // During local use there is no active symlink update, so active/bin
    // must resolve to the selected release rather than <root>/active/bin.
    if let Some(rest) = bin.strip_prefix("active/") {
        release.join(rest)
    } else {
        layout.root.join(bin)
    }
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
    case " $* " in
      *" -g "*|*" --global "*)
        command relo use "$@"
        ;;
      *)
        eval "$(command relo use --shell posix "$@")"
        ;;
    esac
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
    if ($rest -contains "-g" -or $rest -contains "--global") {
      & relo.exe use @rest
    } else {
      Invoke-Expression (& relo.exe use --shell powershell @rest)
    }
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

fn escape_powershell(value: &str) -> String {
    value.replace('\'', "''")
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
