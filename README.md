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
└── relo.yaml
```

Versioned home mode:

```text
<root>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.yaml
```

Release directories must be named as `<semver>` or `<semver>_<label>`, for example `3.9.9`, `3.9.9_arm64`, or `1.12.0_darwin-arm64`. Version comparison uses only the semantic version before `_`.

## Commands

```bash
relo [-d <dir>] init [--force] [--home shared|versioned]
relo [-d <dir>] init [--force] [--path <dir>]
relo [-d <dir>] list
relo [-d <dir>] show [version]
relo [-d <dir>] use [--path <dir>] [version]
relo [-d <dir>] use --shell <posix|powershell|cmd> [version]
relo [-d <dir>] use -g [version]
relo [-d <dir>] print <root|active|release|home|version> [version]
relo [-d <dir>] config [show]
relo init zsh
relo init bash
relo init powershell
relo init cmd
```

If `-d` is omitted, `relo` uses `RELO_CTX` when it is set, otherwise it uses
the current working directory. This keeps one-off scripts compact:

```bash
RELO_CTX=/opt/relo/maven relo init
RELO_CTX=/opt/relo/maven relo use -g 3.9.9
```

## Usage

```bash
mkdir -p ~/Documents/30_Toolchain/Maven
cd ~/Documents/30_Toolchain/Maven
relo init
mkdir -p releases/3.8.8 releases/3.9.9
relo use -g 3.9
relo list
```

When `version` is omitted, `relo use` selects the active release, or `latest`
when no active release exists.

`relo init` refuses to overwrite an existing `relo.yaml` unless `--force` is
passed.

Temporary use prints shell exports:

```bash
eval "$(relo use 3.8)"
```

Install a shell wrapper to make `relo use <version>` affect the current shell directly:

```bash
eval "$(relo init zsh)"
```

Add the same line to `.zshrc` or use `relo init bash` for bash.

On Windows, local use defaults to PowerShell output:

```powershell
Invoke-Expression (relo use 3.8)
```

Install the PowerShell wrapper to make `relo use <version>` affect the current PowerShell session directly:

```powershell
Invoke-Expression (relo init powershell)
```

The default shell output is selected by platform: Windows uses PowerShell, while Linux and macOS use POSIX shell output. You can override it explicitly:

```bash
relo use --shell posix 3.8
relo use --shell powershell 3.8
relo use --shell cmd 3.8
```

## PATH Handling

`relo.yaml` controls how directories are added to `PATH`:

```yaml
path:
  - active/bin
```

All path entries are added before the existing `PATH` so the active release takes precedence. `--path` adds temporary front-of-PATH entries for local use:

```bash
relo init --path active/bin --path tools/bin
relo use 3.9 --path active/sbin --path /opt/tools/bin
```

Relative paths are resolved against the relo root. Absolute paths are allowed. Symbolic prefixes are supported:

```text
active/bin   -> selected release/bin
release/bin  -> selected release/bin
home/bin     -> selected home/bin
root/tools   -> root/tools
tools/bin    -> root/tools/bin
/opt/bin     -> /opt/bin
```

Relative paths containing `..` are rejected.

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

`relo.yaml` is local to each root:

```yaml
name: Maven
home_mode: shared
version_separator: _

path:
  - active/bin

env:
  MAVEN_HOME:
    path: release
  MAVEN_USER_HOME:
    path: home
  JAVA_OPTS:
    value: -Xmx1g

releases:
  - id: 3.9.9
    path:
      - active/sbin
    env:
      JAVA_OPTS:
        value: -Xmx2g
```

`env` values support two explicit forms:

```yaml
env:
  SOME_PATH:
    path: home/config
  SOME_VALUE:
    value: literal-value
```

Release-specific `env` values override global `env` values with the same name. Release-specific `path` entries are added before global path entries. Local `relo use` exports only configured `env` values and `PATH`; it does not inject implicit relo variables. `relo use -g` only updates `active`; temporary `--path` overrides are valid only for local use.
