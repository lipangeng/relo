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

fn run_with_relo_context(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .env("RELO_CONTEXT", root)
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

fn assert_success_output(output: Output) -> (String, String) {
    assert!(
        output.status.success(),
        "expected success\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    (stdout(&output), stderr(&output))
}

fn init(root: &Path) {
    assert_success(run(root, &["init"]));
}

fn mkdir_release(root: &Path, name: &str) {
    fs::create_dir_all(root.join("releases").join(name).join("bin")).unwrap();
}

#[cfg(unix)]
fn assert_active_target(root: &Path, version: &str) {
    assert_eq!(
        fs::read_link(root.join("active")).unwrap(),
        PathBuf::from("releases").join(version)
    );
}

#[cfg(windows)]
fn assert_active_target(root: &Path, version: &str) {
    let active = root.join("active");
    assert!(junction::exists(&active).unwrap());
    assert_eq!(
        fs::canonicalize(junction::get_target(active).unwrap()).unwrap(),
        fs::canonicalize(root.join("releases").join(version)).unwrap()
    );
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

#[cfg(target_os = "macos")]
fn set_quarantine(path: &Path) {
    let output = Command::new("/usr/bin/xattr")
        .args(["-w", "com.apple.quarantine", "relo-test"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
}

#[cfg(target_os = "macos")]
fn has_quarantine(path: &Path) -> bool {
    Command::new("/usr/bin/xattr")
        .args(["-p", "com.apple.quarantine"])
        .arg(path)
        .output()
        .unwrap()
        .status
        .success()
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

#[cfg(target_os = "macos")]
#[test]
fn mac_unblock_removes_only_the_selected_release_quarantine_attributes() {
    let root = temp_root("mac-unblock-selected");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");

    let selected = root.join("releases/1.0.0");
    let selected_bin = selected.join("bin");
    let untouched = root.join("releases/2.0.0");
    set_quarantine(&selected);
    set_quarantine(&selected_bin);
    set_quarantine(&untouched);
    assert!(has_quarantine(&selected));
    assert!(has_quarantine(&selected_bin));
    assert!(has_quarantine(&untouched));

    let (out, err) = assert_success_output(run(&root, &["mac", "unblock", "-v", "1.0"]));

    assert!(out.contains("unblocked: 1.0.0"));
    assert!(err.contains("version: 1.0.0"));
    assert!(err.contains(&format!("release: {}", selected.display())));
    assert!(err.contains("attribute: com.apple.quarantine"));
    assert!(err.contains("recursive: yes"));
    assert!(
        err.contains(&selected_bin.display().to_string()),
        "expected xattr verbose output for selected bin\nstderr:\n{err}"
    );
    assert!(!has_quarantine(&selected));
    assert!(!has_quarantine(&selected_bin));
    assert!(has_quarantine(&untouched));
}

#[cfg(target_os = "macos")]
#[test]
fn mac_unblock_is_idempotent_and_uses_the_default_release() {
    let root = temp_root("mac-unblock-default");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");
    assert_success(run(&root, &["use", "-g", "1.0.0"]));

    let out = assert_success(run(&root, &["mac", "unblock"]));
    assert_eq!(out, "unblocked: 1.0.0\n");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn mac_unblock_reports_unsupported_platform() {
    let root = temp_root("mac-unblock-platform");
    init(&root);
    mkdir_release(&root, "1.0.0");

    let err = assert_failure(run(&root, &["mac", "unblock", "1.0.0"]));
    assert!(err.contains("mac unblock is only supported on macOS"));
}

#[cfg(not(windows))]
#[test]
fn win_env_reports_unsupported_platform_before_loading_context() {
    let root = temp_root("win-env-platform");
    let err = assert_failure(run(&root, &["win", "env", "status"]));
    assert!(err.contains("win env is only supported on Windows"));
    assert!(!err.contains("not a relo context"));
}

#[test]
fn win_env_help_exposes_persistent_environment_commands() {
    let root = temp_root("win-env-help");
    let out = assert_success(run(&root, &["win", "env", "--help"]));
    for command in ["apply", "status", "remove", "prune"] {
        assert!(out.contains(command));
    }
}

#[cfg(windows)]
#[test]
fn win_env_apply_dry_run_does_not_emit_verbatim_paths() {
    let root = temp_root("win-env-no-verbatim-path");
    init(&root);
    mkdir_release(&root, "1.0.0");

    let out = assert_success(run(&root, &["win", "env", "apply", "1.0.0", "--dry-run"]));

    assert!(out.contains(r"\active\bin"));
    assert!(!out.contains(r"\\?\"), "{out}");
}

#[test]
fn init_refuses_to_overwrite_existing_config_by_default() {
    let root = temp_root("init-existing");
    init(&root);
    let config_path = root.join("relo.yaml");
    let original = "name: keep-me\n";
    fs::write(&config_path, original).unwrap();

    let err = assert_failure(run(&root, &["init", "--home", "versioned"]));
    assert!(err.contains("relo.yaml already exists"));
    assert!(err.contains("--force"));
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
}

#[test]
fn init_force_overwrites_existing_config() {
    let root = temp_root("init-force");
    init(&root);
    fs::write(root.join("relo.yaml"), "name: old\n").unwrap();

    assert_success(run(&root, &["init", "--force", "--home", "versioned"]));

    let config = fs::read_to_string(root.join("relo.yaml")).unwrap();
    assert!(config.contains("name: relo-init-force-"));
    assert!(config.contains("home_mode: versioned"));
}

#[test]
fn relo_ctx_env_selects_root_when_dir_option_is_omitted() {
    let root = temp_root("env-root");
    assert_success(run_with_relo_ctx(&root, &["init"]));

    assert!(root.join("relo.yaml").is_file());
    assert!(root.join("releases").is_dir());
}

#[test]
fn relo_context_env_selects_context_when_dir_option_is_omitted() {
    let root = temp_root("env-context");
    assert_success(run_with_relo_context(&root, &["init"]));

    assert!(root.join("relo.yaml").is_file());
    assert!(root.join("releases").is_dir());
}

#[test]
fn relo_context_env_takes_precedence_over_relo_ctx() {
    let ctx_root = temp_root("env-context-precedence");
    let legacy_root = temp_root("env-ctx-ignored");
    let output = Command::new(bin())
        .env("RELO_CONTEXT", &ctx_root)
        .env("RELO_CTX", &legacy_root)
        .arg("init")
        .output()
        .unwrap();
    assert_success(output);

    assert!(ctx_root.join("relo.yaml").is_file());
    assert!(!legacy_root.join("relo.yaml").exists());
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

    let active = root.join("active");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active.join("bin"))
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
fn init_accepts_path_options() {
    let root = temp_root("init-path");
    assert_success(run(
        &root,
        &[
            "init",
            "--path",
            "${relo.release}/bin",
            "--path",
            "tools/bin",
        ],
    ));

    let config = fs::read_to_string(root.join("relo.yaml")).unwrap();
    assert!(config.contains("- ${relo.release}/bin"));
    assert!(config.contains("- tools/bin"));
}

#[test]
fn print_version_resolves_dotted_version_prefixes_by_highest_match() {
    let root = temp_root("version-prefix");
    init(&root);
    for version in [
        "3.5.0",
        "3.5.2",
        "3.5.7",
        "3.6.1",
        "3.9.9",
        "3.10.0",
        "8.1.1.7",
        "8.1.1.10",
        "8.1.1.10.2",
    ] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "3"])),
        "3.10.0\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "3.5"])),
        "3.5.7\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "8.1.1"])),
        "8.1.1.10.2\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "8.1.1.7"])),
        "8.1.1.7\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "8.1.1.10"])),
        "8.1.1.10\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "latest"])),
        "8.1.1.10.2\n"
    );
}

#[test]
fn print_version_accepts_single_and_two_part_versions() {
    let root = temp_root("version-short");
    init(&root);
    for version in ["8", "8.1"] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "8"])),
        "8\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "8.1"])),
        "8.1\n"
    );
}

#[test]
fn print_version_accepts_v_prefixed_versions() {
    let root = temp_root("version-v-prefix");
    init(&root);
    for version in ["v1.2.3", "V1.2.4", "v1.3.0_arm64"] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "v1.2"])),
        "V1.2.4\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "V1.2.3"])),
        "v1.2.3\n"
    );
    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "1.3.0"])),
        "v1.3.0_arm64\n"
    );
    assert_eq!(
        assert_success(run(
            &root,
            &["print", "version", "--version", "v1.3.0_arm64"]
        )),
        "v1.3.0_arm64\n"
    );
}

#[test]
fn print_version_prefers_unlabeled_exact_three_part_match() {
    let root = temp_root("label-prefer-base");
    init(&root);
    for version in ["3.9.9_arm64", "3.9.9", "3.9.9_internal"] {
        mkdir_release(&root, version);
    }

    assert_eq!(
        assert_success(run(&root, &["print", "version", "--version", "3.9.9"])),
        "3.9.9\n"
    );
    assert_eq!(
        assert_success(run(
            &root,
            &["print", "version", "--version", "3.9.9_arm64"]
        )),
        "3.9.9_arm64\n"
    );
}

#[test]
fn print_version_reports_ambiguous_labeled_exact_three_part_match() {
    let root = temp_root("label-ambiguous");
    init(&root);
    for version in ["3.9.9_arm64", "3.9.9_internal"] {
        mkdir_release(&root, version);
    }

    let err = assert_failure(run(&root, &["print", "version", "--version", "3.9.9"]));
    assert!(err.contains("ambiguous release: 3.9.9"));
    assert!(err.contains("3.9.9_arm64"));
    assert!(err.contains("3.9.9_internal"));
}

#[test]
fn print_path_outputs_effective_paths_one_per_line() {
    let root = temp_root("print-path");
    init(&root);
    mkdir_release(&root, "3.8.8");
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: print-path\npath:\n  - ${relo.release}/bin\n  - tools/bin\nreleases:\n  - id: 3.8.8\n    path:\n      - ${relo.release}/sbin\n",
    );

    let release = root.join("releases").join("3.8.8");
    assert_eq!(
        assert_success(run(&root, &["print", "path", "--version", "3.8"])),
        format!(
            "{}\n{}\n{}\n",
            release.join("sbin").display(),
            release.join("bin").display(),
            root.join("tools").join("bin").display()
        )
    );
}

#[test]
fn print_context_and_ctx_output_context_directory() {
    let root = temp_root("print-context");
    init(&root);

    assert_eq!(
        assert_success(run(&root, &["print", "context"])),
        format!("{}\n", root.display())
    );
    assert_eq!(
        assert_success(run(&root, &["print", "ctx"])),
        format!("{}\n", root.display())
    );
}

#[test]
fn print_env_outputs_effective_env_one_per_line() {
    let root = temp_root("print-env");
    init(&root);
    mkdir_release(&root, "3.8.8");
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: print-env\nenv:\n  JAVA_HOME: ${relo.release}\n  JAVA_OPTS: -Xmx1g\nreleases:\n  - id: 3.8.8\n    env:\n      JAVA_OPTS: -Xmx2g\n",
    );

    let release = root.join("releases").join("3.8.8");
    assert_eq!(
        assert_success(run(&root, &["print", "env", "--version", "3.8"])),
        format!("JAVA_HOME={}\nJAVA_OPTS=-Xmx2g\n", release.display())
    );
}

#[test]
fn env_expands_in_layer_order_and_path_uses_final_env() {
    let root = temp_root("env-layer-order");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: env-layer-order\nenv:\n  ROOT: ${relo.release}\n  BIN: ${env.ROOT}/bin\nreleases:\n  - id: 1.0.0\n    env:\n      ROOT: ${relo.home}/jdk\n      RELEASE_BIN: ${env.ROOT}/release-bin\n    path:\n      - ${env.ROOT}/tools\n      - ${env.RELEASE_BIN}\n",
    );

    let release = root.join("releases").join("1.0.0");
    let home = root.join("home");
    assert_eq!(
        assert_success(run(&root, &["print", "env", "--version", "1.0.0"])),
        format!(
            "BIN={}\nROOT={}\nRELEASE_BIN={}\n",
            release.join("bin").display(),
            home.join("jdk").display(),
            home.join("jdk").join("release-bin").display()
        )
    );
    assert_eq!(
        assert_success(run(&root, &["print", "path", "--version", "1.0.0"])),
        format!(
            "{}\n{}\n{}\n",
            home.join("jdk").join("tools").display(),
            home.join("jdk").join("release-bin").display(),
            root.join("active").join("bin").display()
        )
    );
}

#[test]
fn config_expands_context_and_ctx_variables() {
    let root = temp_root("context-vars");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: context-vars\nenv:\n  CONTEXT_HOME: ${relo.context}/cache\n  CTX_HOME: ${relo.ctx}/cache\npath:\n  - ${relo.context}/tools\n  - ${relo.ctx}/bin\n",
    );

    assert_eq!(
        assert_success(run(&root, &["print", "env", "--version", "1.0.0"])),
        format!(
            "CONTEXT_HOME={}\nCTX_HOME={}\n",
            root.join("cache").display(),
            root.join("cache").display()
        )
    );
    assert_eq!(
        assert_success(run(&root, &["print", "path", "--version", "1.0.0"])),
        format!(
            "{}\n{}\n",
            root.join("tools").display(),
            root.join("bin").display()
        )
    );
}

#[test]
fn config_expands_active_variable_to_active_symlink_path() {
    let root = temp_root("active-var");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: active-var\nenv:\n  ACTIVE: ${relo.active}\npath:\n  - ${relo.active}/bin\n",
    );

    assert_eq!(
        assert_success(run(&root, &["print", "env", "--version", "1.0.0"])),
        format!("ACTIVE={}\n", root.join("active").display())
    );
    assert_eq!(
        assert_success(run(&root, &["print", "path", "--version", "1.0.0"])),
        format!("{}\n", root.join("active").join("bin").display())
    );
}

#[test]
fn env_preserves_literals_while_path_resolves_paths() {
    let root = temp_root("env-literal-path-resolve");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: env-literal-path-resolve\nenv:\n  RELATIVE: tools/bin\n  TILDE: ~/cache\npath:\n  - tools/bin\n",
    );

    let home_cache = shellexpand::tilde("~/cache").into_owned();
    let home_cache: PathBuf = Path::new(&home_cache).components().collect();
    assert_eq!(
        assert_success(run(&root, &["print", "env", "--version", "1.0.0"])),
        format!("RELATIVE=tools/bin\nTILDE={}\n", home_cache.display())
    );
    assert_eq!(
        assert_success(run(&root, &["print", "path", "--version", "1.0.0"])),
        format!("{}\n", root.join("tools").join("bin").display())
    );
}

#[test]
fn print_path_without_version_prefers_active_then_latest() {
    let root = temp_root("print-default-release");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");

    assert_eq!(
        assert_success(run(&root, &["print", "path"])),
        format!("{}\n", root.join("active/bin").display())
    );

    assert_success(run(&root, &["use", "-g", "1.0.0"]));
    assert_eq!(
        assert_success(run(&root, &["print", "path"])),
        format!("{}\n", root.join("active/bin").display())
    );
}

#[test]
fn print_rejects_version_for_targets_without_release_context() {
    let root = temp_root("print-root-version");
    init(&root);
    mkdir_release(&root, "1.0.0");

    let err = assert_failure(run(&root, &["print", "context", "--version", "1.0.0"]));
    assert!(err.contains("--version is not valid for print context"));
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
    assert_active_target(&root, "3.9.9");

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
    let active = root.join("active");
    assert!(!out.contains("RELO_ROOT"));
    assert!(!out.contains("RELO_RELEASE"));
    assert!(!out.contains("RELO_HOME"));
    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active.join("bin"))
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
            "--path",
            "${relo.release}/sbin",
            "3.9",
        ],
    ));

    assert!(out.contains(&format!(
        "export PATH=\"{}:{}:$PATH\"",
        shell_escape_path(&release.join("sbin")),
        shell_escape_path(&root.join("active").join("bin"))
    )));
}

#[cfg(not(windows))]
#[test]
fn local_use_can_append_path_entries() {
    let root = temp_root("use-path-append");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let active = root.join("active");
    let out = assert_success(run(
        &root,
        &["use", "--shell", "posix", "--path-append", "3.9"],
    ));

    assert!(out.contains(&format!(
        "export PATH=\"$PATH:{}\"",
        shell_escape_path(&active.join("bin"))
    )));
    assert!(!out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active.join("bin"))
    )));
}

#[cfg(not(windows))]
#[test]
fn local_use_verbose_reports_selected_version_on_stderr() {
    let root = temp_root("use-verbose");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: use-verbose\nenv:\n  MAVEN_HOME: ${relo.release}\npath:\n  - ${relo.release}/bin\n  - tools/bin\n",
    );

    let release = root.join("releases").join("3.9.9");
    let output = run(&root, &["use", "-v", "--shell", "posix", "3.9"]);
    let (out, err) = assert_success_output(output);

    assert!(out.contains(&shell_escape_path(&release.join("bin"))));
    assert!(err.contains("version: 3.9.9"));
    assert!(err.contains(&format!("release: {}", release.display())));
    assert!(err.contains("mode: local"));
    assert!(err.contains("env:"));
    assert!(err.contains(&format!("  MAVEN_HOME={}", release.display())));
    assert!(err.contains("path:"));
    assert!(err.contains(&format!("  {}", release.join("bin").display())));
    assert!(err.contains(&format!("  {}", root.join("tools").join("bin").display())));
}

#[cfg(not(windows))]
#[test]
fn later_local_use_takes_path_precedence() {
    let root = temp_root("use-path-precedence");
    init(&root);
    mkdir_release(&root, "8.0.0");
    mkdir_release(&root, "11.0.0");

    let java8 = root.join("releases").join("8.0.0").join("bin");
    let active = root.join("active").join("bin");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "11"]));

    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active)
    )));
    assert!(!out.contains(&format!(
        "export PATH=\"$PATH:{}\"",
        shell_escape_path(&active)
    )));
    assert!(!out.contains(&shell_escape_path(&java8)));
}

#[cfg(not(windows))]
#[test]
fn config_path_entries_are_prepended() {
    let root = temp_root("config-path-prepend");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: config-path-prepend\npath:\n  - ${relo.release}/sbin\n  - /opt/tools/bin\n",
    );

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains(&format!(
        "export PATH=\"{}:/opt/tools/bin:$PATH\"",
        shell_escape_path(&release.join("sbin"))
    )));
}

#[test]
fn config_env_supports_path_and_literal_values() {
    let root = temp_root("env-path-value");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: env-path-value\nhome_mode: shared\nversion_separator: _\npath:\n  - ${relo.release}/bin\nenv:\n  MAVEN_HOME: ${relo.release}\n  JAVA_OPTS: -Xmx1g\n",
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
fn config_path_and_env_support_yaml_aliases() {
    let root = temp_root("yaml-aliases");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: yaml-aliases\npath:\n  - ${relo.release}/bin\nenv:\n  TOOL_BIN: &tool_bin ${relo.release}/bin\n  TOOL_BIN_COPY: *tool_bin\n",
    );

    let bin = root.join("releases").join("3.9.9").join("bin");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&bin)
    )));
    assert!(out.contains(&format!("export TOOL_BIN=\"{}\"", shell_escape_path(&bin))));
    assert!(out.contains(&format!(
        "export TOOL_BIN_COPY=\"{}\"",
        shell_escape_path(&bin)
    )));
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
        "name: release-override\nhome_mode: shared\nversion_separator: _\npath:\n  - ${relo.release}/bin\n  - /opt/global/bin\nenv:\n  JAVA_OPTS: -Xmx1g\nreleases:\n  - id: 3.9.9\n    path:\n      - ${relo.release}/sbin\n      - /opt/release/bin\n    env:\n      JAVA_OPTS: -Xmx2g\n",
    );

    let release = root.join("releases").join("3.9.9");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "3.9"]));
    assert!(out.contains("export JAVA_OPTS=\"-Xmx2g\""));
    assert!(out.contains(&format!(
        "export PATH=\"{}:/opt/release/bin:{}:/opt/global/bin:$PATH\"",
        shell_escape_path(&release.join("sbin")),
        shell_escape_path(&release.join("bin"))
    )));
}

#[test]
fn global_use_rejects_temporary_path_overrides() {
    let root = temp_root("global-path-override");
    init(&root);
    mkdir_release(&root, "1.0.0");

    let err = assert_failure(run(
        &root,
        &["use", "-g", "--path", "${relo.release}/sbin", "1.0.0"],
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
    assert!(out.contains("export PATH="));
    assert!(!out.contains("export RELO_ROOT="));
    assert!(!out.contains("$env:RELO_ROOT"));
}

#[cfg(windows)]
#[test]
fn local_use_defaults_to_powershell_on_windows() {
    let root = temp_root("default-powershell");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let out = assert_success(run(&root, &["use", "3.9"]));
    assert!(out.contains("$env:PATH = "));
    assert!(!out.contains("$env:RELO_ROOT = '"));
    assert!(!out.contains("export RELO_ROOT="));
}

#[test]
fn local_use_can_output_powershell_script() {
    let root = temp_root("powershell-use");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let active = root.join("active");
    let out = assert_success(run(&root, &["use", "--shell", "powershell", "3.9"]));
    assert!(!out.contains("$env:RELO_ROOT"));
    assert!(!out.contains("$env:RELO_RELEASE"));
    assert!(!out.contains("$env:RELO_HOME"));
    assert!(out.contains(&format!(
        "$env:PATH = '{}' + ';' + $env:PATH",
        powershell_escape_path(&active.join("bin"))
    )));
}

#[test]
fn local_use_can_append_path_entries_in_powershell() {
    let root = temp_root("powershell-append-path");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let active = root.join("active");
    let out = assert_success(run(
        &root,
        &["use", "--shell", "powershell", "--path-append", "3.9"],
    ));
    assert!(out.contains(&format!(
        "$env:PATH = $env:PATH + ';' + '{}'",
        powershell_escape_path(&active.join("bin"))
    )));
}

#[test]
fn local_use_can_output_cmd_script() {
    let root = temp_root("cmd-use");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let active = root.join("active");
    let out = assert_success(run(&root, &["use", "--shell", "cmd", "3.9"]));
    assert!(!out.contains("RELO_ROOT"));
    assert!(!out.contains("RELO_RELEASE"));
    assert!(!out.contains("RELO_HOME"));
    assert!(out.contains(&format!(
        "set \"PATH={};%PATH%\"",
        active.join("bin").display()
    )));
}

#[test]
fn local_use_can_append_path_entries_in_cmd() {
    let root = temp_root("cmd-append-path");
    init(&root);
    mkdir_release(&root, "3.9.9");

    let active = root.join("active");
    let out = assert_success(run(
        &root,
        &["use", "--shell", "cmd", "--path-append", "3.9"],
    ));
    assert!(out.contains(&format!(
        "set \"PATH=%PATH%;{}\"",
        active.join("bin").display()
    )));
}

#[test]
fn versioned_home_prints_and_use_creates_version_home() {
    let root = temp_root("versioned-home");
    assert_success(run(&root, &["init", "--home", "versioned"]));
    mkdir_release(&root, "3.9.9");

    let expected = root.join("homes/3.9.9");
    assert_eq!(
        assert_success(run(&root, &["print", "home", "--version", "3.9"])),
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
    assert!(show.contains(&format!("context:  {}", root.display())));
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
    assert!(!out.contains("case \" $* \""));
}

#[test]
fn init_powershell_generates_wrapper_that_invokes_expression_for_local_use() {
    let root = temp_root("powershell-wrapper");
    let out = assert_success(run(&root, &["init", "powershell"]));

    assert!(out.contains("function relo"));
    assert!(out.contains("Invoke-Expression"));
    assert!(out.contains("--shell powershell"));
    assert!(!out.contains("$rest -contains \"--help\""));
}

#[test]
fn init_cmd_generates_wrapper_for_cmd_shell() {
    let root = temp_root("cmd-wrapper");
    let out = assert_success(run(&root, &["init", "cmd"]));

    assert!(out.contains("doskey relo="));
    assert!(out.contains("--shell cmd"));
}

#[test]
fn use_shell_help_outputs_forwarding_script() {
    let root = temp_root("shell-help");
    let out = assert_success(run(&root, &["use", "--shell", "posix", "--help"]));

    assert!(out.contains("\"use\" \"--help\""));
    assert!(!out.contains("Usage:"));
}

#[test]
fn use_help_outputs_human_readable_help_without_shell() {
    let root = temp_root("use-help");
    let out = assert_success(run(&root, &["use", "--help"]));

    assert!(out.contains("Usage: use"));
    assert!(out.contains("--path-append"));
}

#[test]
fn config_rejects_invalid_shell_env_names() {
    let root = temp_root("invalid-env-name");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: invalid-env-name\nhome_mode: shared\nversion_separator: _\npath:\n  - ${relo.release}/bin\nenv:\n  \"BAD; touch /private/tmp/relo-injected; #\": home\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("invalid env variable name"));
}

#[test]
fn config_rejects_relative_paths_that_escape_context() {
    let root = temp_root("escape-path");
    init(&root);
    mkdir_release(&root, "1.0.0");
    write_config(
        &root,
        "name: escape-path\nhome_mode: shared\nversion_separator: _\npath:\n  - ../outside/bin\nenv:\n  CONFIG_DIR: /tmp/config\n",
    );

    let err = assert_failure(run(&root, &["use", "1.0.0"]));
    assert!(err.contains("relative path must not escape context"));
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
fn local_use_without_version_uses_latest_release() {
    let root = temp_root("local-latest");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");

    let active = root.join("active");
    let out = assert_success(run(&root, &["use", "--shell", "posix"]));
    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active.join("bin"))
    )));
    assert!(!root.join("active").exists());
}

#[test]
fn local_use_without_version_prefers_active_release() {
    let root = temp_root("local-active-default");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");
    assert_success(run(&root, &["use", "-g", "1.0.0"]));

    let active = root.join("active");
    let out = assert_success(run(&root, &["use", "--shell", "posix"]));
    assert!(out.contains(&format!(
        "export PATH=\"{}:$PATH\"",
        shell_escape_path(&active.join("bin"))
    )));
    assert!(!out.contains(&shell_escape_path(
        &root.join("releases").join("2.0.0").join("bin")
    )));
}

#[test]
fn global_use_without_version_uses_latest_release() {
    let root = temp_root("global-latest");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");

    assert_success(run(&root, &["use", "-g"]));

    assert_active_target(&root, "2.0.0");
    assert_eq!(assert_success(run(&root, &["print", "version"])), "2.0.0\n");
}

#[test]
fn global_use_without_version_keeps_active_release() {
    let root = temp_root("global-active-default");
    init(&root);
    mkdir_release(&root, "1.0.0");
    mkdir_release(&root, "2.0.0");
    assert_success(run(&root, &["use", "-g", "1.0.0"]));

    assert_success(run(&root, &["use", "-g"]));

    assert_active_target(&root, "1.0.0");
}

#[cfg(windows)]
#[test]
fn global_use_repairs_a_junction_after_the_context_moves() {
    let root = temp_root("global-moved-context");
    init(&root);
    mkdir_release(&root, "1.0.0");
    assert_success(run(&root, &["use", "-g", "1.0.0"]));

    let moved = root.with_file_name(format!(
        "{}-moved",
        root.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(&root, &moved).unwrap();

    assert_success(run(&moved, &["use", "-g", "latest"]));
    assert_active_target(&moved, "1.0.0");
}

#[test]
fn global_use_verbose_reports_selected_version_on_stderr() {
    let root = temp_root("global-verbose");
    init(&root);
    mkdir_release(&root, "3.9.9");
    write_config(
        &root,
        "name: global-verbose\nenv:\n  MAVEN_HOME: ${relo.release}\npath:\n  - ${relo.release}/bin\n",
    );

    let output = run(&root, &["use", "-g", "-v", "3.9"]);
    let (out, err) = assert_success_output(output);
    let release = root.join("releases").join("3.9.9");

    assert_eq!(out, "");
    assert!(err.contains("version: 3.9.9"));
    assert!(err.contains(&format!("release: {}", release.display())));
    assert!(err.contains("mode: global"));
    assert!(err.contains("env:"));
    assert!(err.contains(&format!("  MAVEN_HOME={}", release.display())));
    assert!(err.contains("path:"));
    assert!(err.contains(&format!("  {}", release.join("bin").display())));
}
