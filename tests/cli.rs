use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> String {
    std::env::var("CARGO_BIN_EXE_relo").unwrap()
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("relo-{name}-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    fs::canonicalize(root).unwrap()
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("-d")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn run_with_relo_ctx(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .env("RELO_CTX", root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace('\r', "")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace('\r', "")
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "expected success\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    stdout(&output)
}

fn assert_failure(output: Output) -> String {
    assert!(
        !output.status.success(),
        "expected failure\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    stderr(&output)
}

fn init(root: &Path) {
    assert_success(run(root, &["init"]));
}

fn mkdir_release(root: &Path, name: &str) {
    fs::create_dir_all(root.join("releases").join(name).join("bin")).unwrap();
}

fn shell_escape_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

fn powershell_escape_path(path: &Path) -> String {
    path.display().to_string().replace('\'', "''")
}

fn write_config(root: &Path, text: &str) {
    fs::write(root.join("relo.yaml"), text).unwrap();
}

#[test]
fn init_creates_shared_layout_and_config() {
    let root = temp_root("init-shared");
    assert_success(run(&root, &["init"]));

    assert!(root.join("releases").is_dir());
    assert!(root.join("home").is_dir());
    assert!(!root.join("active").exists());
    assert!(root.join("relo.yaml").is_file());
    assert!(!root.join("relo.toml").exists());
    let config = fs::read_to_string(root.join("relo.yaml")).unwrap();
    assert!(config.contains("name: relo-init-shared-"));
    assert!(!config.contains("home_mode:"));
    assert!(!config.contains("version_separator:"));
    assert!(!config.contains("path:"));
    assert!(!config.contains("env:"));
    assert!(!config.contains("releases:"));
}

#[test]
fn relo_ctx_env_selects_root_when_dir_option_is_omitted() {
    let root = temp_root("env-root");
    assert_success(run_with_relo_ctx(&root, &["init"]));

    assert!(root.join("relo.yaml").is_file());
    assert!(root.join("releases").is_dir());
}

#[test]
fn dir_option_overrides_relo_ctx_env() {
    let env_root = temp_root("env-root-ignored");
    let arg_root = temp_root("arg-root");
    let output = Command::new(bin())
        .env("RELO_CTX", &env_root)
        .arg("-d")
        .arg(&arg_root)
        .arg("init")
        .output()
        .unwrap();
    assert_success(output);

    assert!(!env_root.join("relo.yaml").exists());
    assert!(arg_root.join("relo.yaml").is_file());
}

#[test]
fn init_config_uses_runtime_defaults_for_omitted_path() {
    let root = temp_root("init-default-path");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains(&format!(
        "export PATH=\"$PATH:{}\"",
        shell_escape_path(&release.join("bin"))
    )));
}

#[test]
fn init_versioned_creates_homes_layout() {
    let root = temp_root("init-versioned");
    assert_success(run(&root, &["init", "--home", "versioned"]));

    assert!(root.join("releases").is_dir());
    assert!(root.join("homes").is_dir());
    assert!(!root.join("home").exists());
    let config = fs::read_to_string(root.join("relo.yaml")).unwrap();
    assert!(config.contains("home_mode: versioned"));
    assert!(!config.contains("version_separator:"));
    assert!(!config.contains("path:"));
}

#[test]
fn init_accepts_path_prepend_and_append_options() {
    let root = temp_root("init-path");
    assert_success(run(
        &root,
        &[
            "init",
            "--path-prepend",
            "active/bin",
            "--path-append",
            "tools/bin",
            "--path",
            "/opt/fallback/bin",
        ],
    ));

    let config = fs::read_to_string(root.join("relo.yaml")).unwrap();
    assert!(config.contains("- active/bin"));
    assert!(config.contains("- tools/bin"));
    assert!(config.contains("- /opt/fallback/bin"));
}

#[test]
fn print_version_resolves_semver_prefixes_by_highest_match() {
    let root = temp_root("version-prefix");
    init(&root);
    for version in ["3.5.0", "3.5.2", "3.5.7", "3.6.1", "3.9.9", "3.10.0"] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "3"])),
        "3.10.0\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "3.5"])),
        "3.5.7\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "latest"])),
        "3.10.0\n"
    );
}

#[test]
fn print_version_prefers_unlabeled_exact_semver_match() {
    let root = temp_root("label-prefer-base");
    init(&root);
    for version in ["3.9.9_arm64", "3.9.9", "3.9.9_internal"] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "3.9.9"])),
        "3.9.9\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "3.9.9_arm64"])),
        "3.9.9_arm64\n"
    );
}

#[test]
fn print_version_reports_ambiguous_labeled_exact_semver_match() {
    let root = temp_root("label-ambiguous");
    init(&root);
    for version in ["3.9.9_arm64", "3.9.9_internal"] {
        mkdir_release(&root, version);
    }

    let err = assert_failure(run(&root, &["print", "version", "3.9.9"]));
    assert!(err.contains("ambiguous release: 3.9.9"));
    assert!(err.contains("3.9.9_arm64"));
    assert!(err.contains("3.9.9_internal"));
}

#[test]
fn global_use_updates_active_and_list_marks_it() {
    let root = temp_root("global-use");
    init(&root);
    for version in ["3.8.8", "3.9.9"] {
        mkdir_release(&root, version);
    }

    assert_success(run(&root, &["use", "-g", "3.9"]));
    assert_eq!(assert_success(run(&root, &["print", "version"])), "3.9.9\n");
    assert_eq!(
        fs::read_link(root.join("active")).unwrap(),
        PathBuf::from("releases/3.9.9")
    );

    let list = assert_success(run(&root, &["list"]));
    assert!(list.contains("  3.8.8"));
    assert!(list.contains("* 3.9.9"));
}

#[test]
fn local_use_outputs_shell_exports_without_modifying_active() {
    let root = temp_root("local-use");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    let release = root.join("releases").join("3.9.9");
    assert!(out.contains(&format!(
        "export RELO_ROOT=\"{}\"",
        shell_escape_path(&root)
    )));
    assert!(out.contains(&format!(
        "export RELO_RELEASE=\"{}\"",
        shell_escape_path(&release)
    )));
    assert!(out.contains(&format!(
        "export RELO_HOME=\"{}\"",
        shell_escape_path(&root.join("home"))
    )));
    assert!(out.contains(&format!(
        "export PATH=\"$PATH:{}\"",
        shell_escape_path(&release.join("bin"))
    )));
    assert!(!root.join("active").exists());
}

#[cfg(not(windows))]
#[test]
fn local_use_accepts_temporary_path_overrides() {
    let root = temp_root("use-path-overrides");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(
        &root,
        &[
            "use",
            "--shell",
            "posix",
            "--path-prepend",
            "active/sbin",
            "--path",
            "/opt/fallback/bin",
            "3.9",
        ],
    ));

    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&release.join("sbin"))
    )));
    assert!(out.contains(&format!(
        "export PATH=\"$PATH:{}:/opt/fallback/bin\"",
        shell_escape_path(&release.join("bin"))
    )));
}

#[test]
fn config_env_supports_path_and_literal_values() {
    let root = temp_root("env-path-value");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: env-path-value\nhome_mode: shared\nversion_separator: _\npath:\n  prepend:\n    - active/bin\n  append: []\nenv:\n  MAVEN_HOME:\n    path: release\n  JAVA_OPTS:\n    value: -Xmx1g\n",
    );

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains(&format!(
        "export MAVEN_HOME=\"{}\"",
        shell_escape_path(&release)
    )));
    assert!(out.contains("export JAVA_OPTS=\"-Xmx1g\""));
}

#[cfg(not(windows))]
#[test]
fn release_specific_config_overrides_env_and_extends_path() {
    let root = temp_root("release-override");
    init(&root);
    mkdir_release(&root, "3.8.8");
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: release-override\nhome_mode: shared\nversion_separator: _\npath:\n  prepend:\n    - active/bin\n  append:\n    - /opt/global/bin\nenv:\n  JAVA_OPTS:\n    value: -Xmx1g\nreleases:\n  - id: 3.9.9\n    path:\n      prepend:\n        - active/sbin\n      append:\n        - /opt/release/bin\n    env:\n      JAVA_OPTS:\n        value: -Xmx2g\n",
    );

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains("export JAVA_OPTS=\"-Xmx2g\""));
    assert!(out.contains(&format!(
        "export PATH=\"{}:{}:$PATH\"",
        shell_escape_path(&release.join("sbin")),
        shell_escape_path(&release.join("bin"))
    )));
    assert!(out.contains("export PATH=\"$PATH:/opt/global/bin:/opt/release/bin\""));
}

#[test]
fn global_use_rejects_temporary_path_overrides() {
    let root = temp_root("global-path-override");
    init(&root);
    mkdir_release(&root, "1.0.0");

    let err = assert_failure(run(
        &root,
        &["use", "-g", "--path-prepend", "active/sbin", "1.0.0"],
    ));
    assert!(err.contains("path overrides are only valid for local use"));
}

#[cfg(not(windows))]
#[test]
fn local_use_defaults_to_posix_on_non_windows() {
    let root = temp_root("default-posix");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let out = assert_success(run(&root, &["use", "3.9"]));
    assert!(out.contains("export RELO_ROOT="));
    assert!(!out.contains("$env:RELO_ROOT"));
}

#[cfg(windows)]
#[test]
fn local_use_defaults_to_powershell_on_windows() {
    let root = temp_root("default-powershell");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let out = assert_success(run(&root, &["use", "3.9"]));
    assert!(out.contains("$env:RELO_ROOT = '"));
    assert!(!out.contains("export RELO_ROOT="));
}

#[test]
fn local_use_can_output_powershell_script() {
    let root = temp_root("powershell-use");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "powershell", "3.9"]));
    assert!(out.contains(&format!(
        "$env:RELO_ROOT = '{}'",
        powershell_escape_path(&root)
    )));
    assert!(out.contains(&format!(
        "$env:RELO_RELEASE = '{}'",
        powershell_escape_path(&release)
    )));
    assert!(out.contains(&format!(
        "$env:RELO_HOME = '{}'",
        powershell_escape_path(&root.join("home"))
    )));
    assert!(out.contains(&format!(
        "$env:PATH = $env:PATH + ';' + '{}'",
        powershell_escape_path(&release.join("bin"))
    )));
}

#[test]
fn local_use_can_output_cmd_script() {
    let root = temp_root("cmd-use");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "cmd", "3.9"]));
    assert!(out.contains(&format!("set \"RELO_ROOT={}\"", root.display())));
    assert!(out.contains(&format!("set \"RELO_RELEASE={}\"", release.display())));
    assert!(out.contains(&format!(
        "set \"RELO_HOME={}\"",
        root.join("home").display()
    )));
    assert!(out.contains(&format!(
        "set \"PATH=%PATH%;{}\"",
        release.join("bin").display()
    )));
}

#[test]
fn versioned_home_prints_and_use_creates_version_home() {
    let root = temp_root("versioned-home");
    assert_success(run(&root, &["init", "--home", "versioned"]));
    mkdir_release(&root, "3.9.9");

    let expected = root.join("homes/3.9.9");
    assert_eq!(
        assert_success(run(&root, &["print", "home", "3.9"])),
        format!("{}\n", expected.display())
    );
    assert!(!expected.exists());

    assert_success(run(&root, &["use", "3.9"]));
    assert!(expected.is_dir());
}

#[test]
fn show_and_config_are_human_readable() {
    let root = temp_root("show-config");
    init(&root);
    mkdir_release(&root, "3.9.9");
    assert_success(run(&root, &["use", "-g", "3.9"]));

    let show = assert_success(run(&root, &["show"]));
    assert!(show.contains(&format!("root:     {}", root.display())));
    assert!(show.contains("active:   3.9.9"));
    assert!(show.contains("mode:     shared"));
    assert!(show.contains("releases: 1"));

    let config = assert_success(run(&root, &["config"]));
    assert!(config.contains("name: relo-show-config-"));
    assert!(!config.contains("home_mode: shared"));
}

#[test]
fn init_shell_generates_wrapper_that_evals_local_use_only() {
    let root = temp_root("shell-wrapper");
    let out = assert_success(run(&root, &["init", "zsh"]));

    assert!(out.contains("relo()"));
    assert!(out.contains("eval \"$(command relo use --shell posix \"$@\")\""));
    assert!(out.contains("command relo use \"$@\""));
}

#[test]
fn init_powershell_generates_wrapper_that_invokes_expression_for_local_use() {
    let root = temp_root("powershell-wrapper");
    let out = assert_success(run(&root, &["init", "powershell"]));

    assert!(out.contains("function relo"));
    assert!(out.contains("Invoke-Expression"));
    assert!(out.contains("--shell powershell"));
}

#[test]
fn init_cmd_generates_wrapper_for_cmd_shell() {
    let root = temp_root("cmd-wrapper");
    let out = assert_success(run(&root, &["init", "cmd"]));

    assert!(out.contains("doskey relo="));
    assert!(out.contains("--shell cmd"));
}

#[test]
fn config_rejects_invalid_shell_env_names() {
    let root = temp_root("invalid-env-name");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: invalid-env-name\nhome_mode: shared\nversion_separator: _\npath:\n  prepend:\n    - active/bin\n  append: []\nenv:\n  \"BAD; touch /private/tmp/relo-injected; #\":\n    path: home\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("invalid env variable name"));
}

#[test]
fn config_rejects_relative_paths_that_escape_root() {
    let root = temp_root("escape-path");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: escape-path\nhome_mode: shared\nversion_separator: _\npath:\n  prepend:\n    - ../outside/bin\n  append: []\nenv:\n  CONFIG_DIR:\n    path: /tmp/config\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("relative path must not escape root"));
}

#[cfg(unix)]
#[test]
fn active_must_point_to_releases_relative_target() {
    let root = temp_root("external-active");
    init(&root);
    mkdir_release(&root, "1.0.0");
    std::os::unix::fs::symlink("/tmp/not-a-relo-root/1.0.0", root.join("active")).unwrap();

    let err = assert_failure(run(&root, &["print", "version"]));
    assert!(err.contains("active points to invalid target"));
}

#[test]
fn list_marks_invalid_release_directories_without_failing() {
    let root = temp_root("invalid-list");
    init(&root);
    mkdir_release(&root, "1.0.0");
    fs::create_dir_all(root.join("releases/not-a-version")).unwrap();

    let out = assert_success(run(&root, &["list"]));
    assert!(out.contains("  1.0.0"));
    assert!(out.contains("! not-a-version (invalid)"));
}

#[test]
fn global_use_requires_explicit_version() {
    let root = temp_root("global-requires-version");
    init(&root);
    mkdir_release(&root, "1.0.0");
    assert_success(run(&root, &["use", "-g", "1.0.0"]));

    let err = assert_failure(run(&root, &["use", "-g"]));
    assert!(err.contains("global use requires a version"));
}
