# relo

中文: [README.zh-CN.md](README.zh-CN.md)

`relo` is Rarely Labs' local release layout manager, short for Release Locator.

It targets a deliberately manual software-management workflow: you already prefer to download, extract, and place SDKs, CLIs, databases, build tools, or other versioned software yourself, but still want a simple way to record versions, switch the active version, and prepare the matching `PATH` and environment variables for a shell.

Package managers and SDK managers can provide fuller install, upgrade, and switching workflows, but they also bring their own default directories, implicit configuration, and state that is not always easy to inspect directly. `relo` is not a replacement for those tools; it is a lightweight, limited-scope helper for users who prefer transparent directory layouts, manual control over installed contents, and a cleaner environment model.

It manages user-provided software release directories. It does not download, install, upgrade, remove, scan contexts, or maintain a registry.

It is suitable for tools such as JDK, Maven, Gradle, Node, Go, protoc, internal CLIs, and versioned binary distributions.

A relo context is one managed software-release directory. `context` is the primary term; `ctx` is the supported short form.

## Layout

Shared home mode:

```text
<context>/
├── active -> releases/<version>
├── releases/
├── home/
└── relo.yaml
```

Versioned home mode:

```text
<context>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.yaml
```

Release directories must be named as `<version>` or `<version>_<label>`, where `<version>` is a dotted numeric version with one or more parts and may start with `v` or `V`. Examples: `8`, `8.1`, `v3.9.9`, `8.1.1.7`, `3.9.9_arm64`, or `V1.12.0_darwin-arm64`. Version comparison uses only the numeric version before `_`, ignoring an optional leading `v` or `V`.

## Commands

```bash
relo [-d <dir>] init [--force] [--home shared|versioned]
relo [-d <dir>] init [--force] [--path <dir>]
relo [-d <dir>] list
relo [-d <dir>] show [version]
relo [-d <dir>] use [-v] [--path <dir>] [--path-append] [version]
relo [-d <dir>] use --shell <posix|powershell|cmd> [version]
relo [-d <dir>] use -g [version]
relo [-d <dir>] print <context|ctx|active|release|home|version|path|env> [--version <version>]
relo [-d <dir>] config [show]
relo init zsh
relo init bash
relo init powershell
relo init cmd
```

`-d` selects the context directory. If `-d` is omitted, `relo` uses
`RELO_CONTEXT` when it is set, then `RELO_CTX`, otherwise it uses the current
working directory. This keeps one-off scripts compact:

```bash
RELO_CONTEXT=/opt/relo/maven relo init
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

You can also use a downloaded `relo` binary to initialize a context for managing `relo` itself, then move that binary into the managed layout:

```bash
mkdir -p ~/Documents/30_Toolchain/relo
./relo -d ~/Documents/30_Toolchain/relo init
mkdir -p ~/Documents/30_Toolchain/relo/releases/0.1.5/bin
cp ./relo ~/Documents/30_Toolchain/relo/releases/0.1.5/bin/relo
~/Documents/30_Toolchain/relo/releases/0.1.5/bin/relo -d ~/Documents/30_Toolchain/relo use -g 0.1.5
```

After that, add `~/Documents/30_Toolchain/relo/active/bin` to your shell `PATH`, or invoke `relo` from that active path.

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
  - ${relo.active}/bin
```

By default, path entries are added before the existing `PATH` so the active release takes precedence. `--path-append` adds them after the existing `PATH` instead. `--path` adds temporary entries for local use:

```bash
relo init --path '${relo.release}/bin' --path tools/bin
relo use 3.9 --path '${relo.release}/sbin' --path /opt/tools/bin
relo use 3.9 --path-append
```

Pass `-v` to print the selected version, release path, mode, effective env,
and effective path entries to stderr without changing the shell script printed
to stdout.

`print path` emits one resolved path per line, and `print env` emits one
`KEY=value` pair per line:

```bash
relo print path --version 3.9
relo print env --version 3.9
```

For POSIX shells on macOS/Linux, `PATH` entries are separated by `:`. The
`paste -sd: -` command joins newline-delimited paths from stdin into one
colon-delimited line:

```bash
export PATH="$(relo print path | paste -sd: -):$PATH"
```

PowerShell uses `;` as the path separator:

```powershell
$env:PATH = ((relo print path) -join ';') + ';' + $env:PATH
```

For simple environment values without spaces or shell metacharacters, `xargs`
can pass `KEY=value` pairs to `export`:

```bash
export $(relo print env | xargs)
```

If values may contain spaces, use a line-preserving loop instead so each
`KEY=value` line is exported as one argument:

```bash
while IFS= read -r entry; do export "$entry"; done < <(relo print env)
```

Relative paths are resolved against the relo context. Absolute paths are allowed. Use variables when a path should point at the selected release, home, active symlink, or context directory:

```text
${relo.release}/bin  -> selected release/bin
${relo.home}/bin     -> selected home/bin
${relo.active}/bin   -> active symlink/bin
${relo.context}/bin  -> context/bin
${relo.ctx}/bin      -> context/bin
tools/bin            -> context/tools/bin
/opt/bin             -> /opt/bin
```

Relative paths containing `..` are rejected.

## Version Resolution

Supported expressions:

```text
latest
3
3.5
3.5.0
8.1.1.7
8.1.1.10.2
3.5.0_arm64
```

Numeric expressions can use any number of dotted parts and choose the highest matching dotted numeric version unless they exactly match a full release directory name. Exact full directory names match directly. If `3.9.9`, `3.9.9_arm64`, and `3.9.9_internal` exist, `3.9.9` resolves to the unlabeled release. If only labeled variants exist, `3.9.9` is ambiguous and the full release name must be specified.

## Configuration

`relo.yaml` is local to each context:

```yaml
name: Maven
home_mode: shared
version_separator: _

path:
  - ${relo.active}/bin

env:
  MAVEN_HOME: ${relo.release}
  MAVEN_BIN: ${env.MAVEN_HOME}/bin
  MAVEN_USER_HOME: ${relo.home}
  JAVA_OPTS: -Xmx1g

releases:
  - id: 3.9.9
    path:
      - ${env.MAVEN_BIN}
    env:
      JAVA_OPTS: -Xmx2g
```

`env` values are strings. `env` and `path` values support variable expansion:

```text
${relo.context}    relo context directory
${relo.ctx}        relo context directory
${relo.active}     active symlink path
${relo.release}    selected release directory
${relo.home}       selected home directory
${relo.version}    selected release id
${env.NAME}        previously expanded configured env value
${sys.NAME}        inherited system environment variable
```

`env` is expanded in order. Global entries are expanded first; release-specific entries are expanded after them. A later env entry can reference earlier entries through `${env.NAME}`. Release-specific `env` values override global values with the same name for later expansion and final output.

`${relo.root}` is kept as a compatibility alias for `${relo.context}`, but new configuration should use `${relo.context}` or `${relo.ctx}`.

`${relo.active}` is the active symlink path itself, for example `<context>/active`. It is not the resolved release directory; use `${relo.release}` for the selected release directory.

Release-specific `path` entries are added before global path entries. `path` is expanded after env, so it can reference final `${env.NAME}` values. Path entries may be absolute or relative; relative path entries are resolved under the relo context. Leading `~` is expanded to the current user's home directory in both env and path values. Env values are not otherwise treated as paths. Local `relo use` exports only configured `env` values and `PATH`; it does not inject implicit relo variables. `relo use -g` only updates `active`; temporary `--path` overrides are valid only for local use.
