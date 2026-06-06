# relo

中文: [README.zh-CN.md](README.zh-CN.md)

`relo` is Rarely Labs' local release layout manager, short for Release Locator.

It manages user-provided software release directories. It does not download, install, upgrade, remove, scan roots, or maintain a registry.

## Layout

Shared home mode:

```text
<root>/
├── active -> releases/<version>
├── releases/
├── home/
└── relo.toml
```

Versioned home mode:

```text
<root>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.toml
```

Release directories must be named as `<semver>` or `<semver>_<label>`, for example `3.9.9`, `3.9.9_arm64`, or `1.12.0_darwin-arm64`. Version comparison uses only the semantic version before `_`.

## Commands

```bash
relo [-d <dir>] init [--home shared|versioned]
relo [-d <dir>] list
relo [-d <dir>] show [version]
relo [-d <dir>] use [version]
relo [-d <dir>] use -g <version>
relo [-d <dir>] print <root|active|release|home|version> [version]
relo [-d <dir>] config [show]
relo init zsh
relo init bash
```

If `-d` is omitted, `relo` uses the current working directory.

## Usage

```bash
mkdir -p ~/Documents/30_Toolchain/Maven
cd ~/Documents/30_Toolchain/Maven
relo init
mkdir -p releases/3.8.8 releases/3.9.9
relo use -g 3.9
relo list
```

Temporary use prints shell exports:

```bash
eval "$(relo use 3.8)"
```

Install a shell wrapper to make `relo use <version>` affect the current shell directly:

```bash
eval "$(relo init zsh)"
```

Add the same line to `.zshrc` or use `relo init bash` for bash.

## Version Resolution

Supported expressions:

```text
latest
3
3.5
3.5.0
3.5.0_arm64
```

Prefix expressions choose the highest matching semantic version. Exact full directory names match directly. If `3.9.9`, `3.9.9_arm64`, and `3.9.9_internal` exist, `3.9.9` resolves to the unlabeled release. If only labeled variants exist, `3.9.9` is ambiguous and the full release name must be specified.

## Configuration

`relo.toml` is local to each root:

```toml
name = "Maven"
home_mode = "shared"
version_separator = "_"
bin = ["active/bin"]

[env]
MAVEN_HOME = "active"
MAVEN_USER_HOME = "home"
```

`home_mode` is `shared` or `versioned`. `version_separator` currently supports `_` only.
