# relo

`relo` 是 Rarely Labs 的本地软件 release 布局管理工具，全称是 Release Locator。

它只管理用户指定目录中的本地软件版本布局。它不是包管理器，不负责下载、安装、升级、卸载软件，也不维护 registry，不扫描根目录。

English: [README.md](README.md)

## 目录结构

共享 home 模式：

```text
<root>/
├── active -> releases/<version>
├── releases/
├── home/
└── relo.yaml
```

版本独立 home 模式：

```text
<root>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.yaml
```

release 目录名必须是 `<semver>` 或 `<semver>_<label>`，例如 `3.9.9`、`3.9.9_arm64`、`1.12.0_darwin-arm64`。版本比较只使用 `_` 前面的语义化版本部分。

## 命令

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

如果不传 `-d`，`relo` 会优先使用 `RELO_CTX`，未设置时才使用当前工作目录。
这能让一次性脚本更简洁：

```bash
RELO_CTX=/opt/relo/maven relo init
RELO_CTX=/opt/relo/maven relo use -g 3.9.9
```

## 快速开始

```bash
mkdir -p ~/Documents/30_Toolchain/Maven
cd ~/Documents/30_Toolchain/Maven
relo init
mkdir -p releases/3.8.8 releases/3.9.9
relo use -g 3.9
relo list
```

如果省略 `version`，`relo use` 默认选择 active release；如果还没有
active，则选择 `latest`。

如果 `relo.yaml` 已存在，`relo init` 默认不会覆盖；需要覆盖时显式传
`--force`。

`relo use -g [version]` 会更新 `active` 软链接：

```text
active -> releases/3.9.9
```

临时使用某个版本时，`relo use <version>` 会输出 shell 脚本：

```bash
eval "$(relo use 3.8)"
```

也可以安装 shell wrapper，让 `relo use <version>` 直接影响当前 shell：

```bash
eval "$(relo init zsh)"
```

bash 用户使用：

```bash
eval "$(relo init bash)"
```

Windows 下，临时使用默认输出 PowerShell 脚本：

```powershell
Invoke-Expression (relo use 3.8)
```

也可以安装 PowerShell wrapper，让 `relo use <version>` 直接影响当前 PowerShell 会话：

```powershell
Invoke-Expression (relo init powershell)
```

默认 shell 输出会按平台自动选择：Windows 使用 PowerShell，Linux 和 macOS 使用 POSIX shell。也可以显式指定：

```bash
relo use --shell posix 3.8
relo use --shell powershell 3.8
relo use --shell cmd 3.8
```

## PATH 处理

`relo.yaml` 控制哪些目录加入 `PATH`：

```yaml
path:
  - active/bin
```

所有 path 条目都会放在原始 `PATH` 前面，确保当前激活的 release 优先。`--path` 用于 local use 的临时前置路径：

```bash
relo init --path active/bin --path tools/bin
relo use 3.9 --path active/sbin --path /opt/tools/bin
```

相对路径会相对于 relo root 解析，绝对路径会保持原样。支持以下符号前缀：

```text
active/bin   -> 当前选择的 release/bin
release/bin  -> 当前选择的 release/bin
home/bin     -> 当前 home/bin
root/tools   -> root/tools
tools/bin    -> root/tools/bin
/opt/bin     -> /opt/bin
```

包含 `..` 的相对路径会被拒绝。

## 版本解析

支持的版本表达式：

```text
latest
3
3.5
3.5.0
3.5.0_arm64
```

解析规则：

- `latest` 选择最高语义化版本。
- `3` 选择主版本为 `3` 的最高版本。
- `3.5` 选择 `3.5.x` 中的最高版本。
- `3.5.0` 精确匹配语义化版本。
- `3.5.0_arm64` 精确匹配完整 release 目录名。

如果同时存在 `3.9.9`、`3.9.9_arm64`、`3.9.9_internal`，表达式 `3.9.9` 会优先选择无 label 的 `3.9.9`。

如果只存在 `3.9.9_arm64` 和 `3.9.9_internal`，表达式 `3.9.9` 会报歧义错误，用户需要指定完整 release 名称。

## 配置

每个 root 使用自己的 `relo.yaml`：

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

`env` 支持两种明确写法：

```yaml
env:
  SOME_PATH:
    path: home/config
  SOME_VALUE:
    value: literal-value
```

版本级 `env` 会覆盖同名全局 `env`。版本级 `path` 会排在全局 `path` 前面。local `relo use` 只导出配置里的 `env` 和 `PATH`，不会隐式注入 relo 内部变量。`relo use -g` 只更新 `active`，临时 `--path` 覆盖只允许用于 local use。

## 设计边界

`relo` 第一版只做 release 布局定位和切换，不实现：

- install
- download
- upgrade
- remove
- scan
- registry
- plugin
- shim
- project local version
