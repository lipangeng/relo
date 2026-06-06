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
└── relo.toml
```

版本独立 home 模式：

```text
<root>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.toml
```

release 目录名必须是 `<semver>` 或 `<semver>_<label>`，例如 `3.9.9`、`3.9.9_arm64`、`1.12.0_darwin-arm64`。版本比较只使用 `_` 前面的语义化版本部分。

## 命令

```bash
relo [-d <dir>] init [--home shared|versioned]
relo [-d <dir>] list
relo [-d <dir>] show [version]
relo [-d <dir>] use [version]
relo [-d <dir>] use --shell <posix|powershell|cmd> [version]
relo [-d <dir>] use -g <version>
relo [-d <dir>] print <root|active|release|home|version> [version]
relo [-d <dir>] config [show]
relo init zsh
relo init bash
relo init powershell
relo init cmd
```

如果不传 `-d`，默认使用当前工作目录。

## 快速开始

```bash
mkdir -p ~/Documents/30_Toolchain/Maven
cd ~/Documents/30_Toolchain/Maven
relo init
mkdir -p releases/3.8.8 releases/3.9.9
relo use -g 3.9
relo list
```

`relo use -g <version>` 会更新 `active` 软链接：

```text
active -> releases/3.9.9
```

临时使用某个版本时，`relo use <version>` 会输出 shell export：

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

## home 模式

共享 home：

```toml
home_mode = "shared"
```

所有版本共用：

```text
<root>/home
```

版本独立 home：

```toml
home_mode = "versioned"
```

每个版本使用独立目录：

```text
<root>/homes/<version>
```

`print home` 只输出路径，不强制创建目录。`use` 会自动创建对应 home 目录。

## 配置

每个 root 使用自己的 `relo.toml`：

```toml
name = "Maven"
home_mode = "shared"
version_separator = "_"
bin = ["active/bin"]

[env]
MAVEN_HOME = "active"
MAVEN_USER_HOME = "home"
```

字段说明：

- `name`：显示名称。
- `home_mode`：`shared` 或 `versioned`。
- `version_separator`：版本号和 label 的分隔符，当前只支持 `_`。
- `bin`：临时 use 时加入 `PATH` 的目录，路径相对于 `<root>`。
- `[env]`：临时 use 时输出的环境变量映射。

`[env]` 的值可以是：

```text
root
active
release
home
```

也可以是相对路径，例如：

```toml
CONFIG_DIR = "home/config"
```

输出时会解析为绝对路径。

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
