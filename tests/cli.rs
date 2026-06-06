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

fn append_config(root: &Path, text: &str) {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(root.join("relo.toml"))
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn write_config(root: &Path, text: &str) {
    fs::write(root.join("relo.toml"), text).unwrap();
}

#[test]
fn init_creates_shared_layout_and_config() {
    let root = temp_root("init-shared");
    assert_success(run(&root, &["init"]));

    assert!(root.join("releases").is_dir());
    assert!(root.join("home").is_dir());
    assert!(!root.join("active").exists());
    let config = fs::read_to_string(root.join("relo.toml")).unwrap();
    assert!(config.contains("home_mode = \"shared\""));
}

#[test]
fn init_versioned_creates_homes_layout() {
    let root = temp_root("init-versioned");
    assert_success(run(&root, &["init", "--home", "versioned"]));

    assert!(root.join("releases").is_dir());
    assert!(root.join("homes").is_dir());
    assert!(!root.join("home").exists());
    let config = fs::read_to_string(root.join("relo.toml")).unwrap();
    assert!(config.contains("home_mode = \"versioned\""));
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

    let out = assert_success(run(&root, &["use", "3.9"]));
    assert!(out.contains(&format!(
        "export RELO_ROOT=\"{}\"",
        shell_escape_path(&root)
    )));
    assert!(out.contains(&format!(
        "export RELO_RELEASE=\"{}\"",
        shell_escape_path(&root.join("releases/3.9.9"))
    )));
    assert!(out.contains(&format!(
        "export RELO_HOME=\"{}\"",
        shell_escape_path(&root.join("home"))
    )));
    assert!(out.contains(&format!(
        "export PATH=\"{}/bin:$PATH\"",
        shell_escape_path(&root.join("releases/3.9.9"))
    )));
    assert!(!root.join("active").exists());
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
    assert!(config.contains("home_mode = \"shared\""));
}

#[test]
fn init_shell_generates_wrapper_that_evals_local_use_only() {
    let root = temp_root("shell-wrapper");
    let out = assert_success(run(&root, &["init", "zsh"]));

    assert!(out.contains("relo()"));
    assert!(out.contains("eval \"$(command relo use \"$@\")\""));
    assert!(out.contains("command relo use \"$@\""));
}

#[test]
fn config_rejects_invalid_shell_env_names() {
    let root = temp_root("invalid-env-name");
    init(&root);
    mkdir_release(&root, "1.0.0");
    append_config(
        &root,
        "\n\"BAD; touch /private/tmp/relo-injected; #\" = \"home\"\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("invalid env variable name"));
}

#[test]
fn config_rejects_paths_that_escape_root() {
    let root = temp_root("escape-path");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name = \"escape-path\"\nhome_mode = \"shared\"\nversion_separator = \"_\"\nbin = [\"active/bin\", \"../outside/bin\"]\n\n[env]\nCONFIG_DIR = \"/tmp/config\"\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("path must be relative to root"));
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
