use crate::layout::Layout;
use anyhow::Result;

pub fn exports(layout: &Layout, release_id: &str) -> Result<String> {
    let release = layout.release_path(release_id);
    let home = layout.home_for(release_id);
    let mut out = String::new();
    out.push_str(&format!(
        "export RELO_ROOT=\"{}\"\n",
        escape(&layout.root.display().to_string())
    ));
    out.push_str(&format!(
        "export RELO_RELEASE=\"{}\"\n",
        escape(&release.display().to_string())
    ));
    out.push_str(&format!(
        "export RELO_HOME=\"{}\"\n",
        escape(&home.display().to_string())
    ));

    for (name, value) in &layout.config.env {
        let path = layout.env_path(value, release_id);
        out.push_str(&format!(
            "export {name}=\"{}\"\n",
            escape(&path.display().to_string())
        ));
    }

    for bin in &layout.config.bin {
        // During local use there is no active symlink update, so active/bin
        // must resolve to the selected release rather than <root>/active/bin.
        let bin_path = if let Some(rest) = bin.strip_prefix("active/") {
            release.join(rest)
        } else {
            layout.root.join(bin)
        };
        out.push_str(&format!(
            "export PATH=\"{}:$PATH\"\n",
            escape(&bin_path.display().to_string())
        ));
    }

    Ok(out)
}

pub fn wrapper() -> &'static str {
    r#"relo() {
  if [ "$1" = "use" ]; then
    shift
    case " $* " in
      *" -g "*|*" --global "*)
        command relo use "$@"
        ;;
      *)
        eval "$(command relo use "$@")"
        ;;
    esac
  else
    command relo "$@"
  fi
}
"#
}

fn escape(value: &str) -> String {
    // The generated script is evaluated by the user's shell, so escape only
    // the characters that can break a double-quoted export value.
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}
