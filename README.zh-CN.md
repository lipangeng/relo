# relo

English: [README.md](README.md)

`relo` 是 Rarely Labs 的本地软件 release 布局管理工具，全称是 Release Locator。

它面向一种偏手动的软件管理场景：你已经习惯自己下载、解压、摆放 SDK、CLI、数据库、构建工具或其他按版本分发的软件，但仍然需要一个简单方式记录版本、切换当前版本，并为 shell 准备对应的 `PATH` 和环境变量。

很多包管理器和 SDK 管理工具都能完成更完整的安装、升级和切换流程，但它们通常也会带来各自的默认目录、隐式配置和不容易直接观察的状态。`relo` 不是这些工具的替代品；它只是给偏好透明目录结构、手动掌控安装内容、对环境有一点“洁癖”的用户提供一个轻量化、有限范围的辅助工具。

它只管理用户已经放到本地目录里的软件版本布局。它不会下载、安装、升级、删除软件，也不会扫描 context。只有显式执行 `relo win env` 时才会登记 Windows 持久环境变量。

适合用来管理这类工具：JDK、Maven、Gradle、Node、Go、protoc、内部 CLI、按版本分发的二进制工具包。

## 安装与构建

当前仓库是 Rust 项目，可以直接从源码构建：

```bash
cargo build --release
```

构建后的二进制位于：

```text
target/release/relo
```

GitHub Release 分别提供 Linux、macOS 和 Windows 的 `x64` 与 `arm64` 压缩包，
统一命名为 `relo-<os>-<arch>`。Windows 发布二进制会静态链接 MSVC 运行库，
不要求用户另外安装 Visual C++ Redistributable。

开发时可直接运行：

```bash
cargo run -- --help
cargo run -- -d /opt/relo/maven init
```

如果要全局使用，把 `relo` 放到一个已经在 `PATH` 中的目录即可。

## 核心概念

一个 relo context 是一个独立的软件版本管理目录，里面包含 release 目录、配置文件和可选的 active 指针。`context` 是正式术语，`ctx` 是官方缩写。

- `context`：某个工具的管理目录，例如 `/opt/relo/maven`。
- `releases/`：用户自己放置 release 的目录，`relo` 不负责下载或解压。
- `active`：指向当前全局激活 release 的符号链接。
- `home` / `homes/`：给工具使用的持久化 home 目录。
- `relo.yaml`：该 context 的本地配置文件。

`relo use -g <version>` 只更新 `active`。`relo use <version>` 默认只输出 shell 脚本，用于临时修改当前 shell 的环境变量和 `PATH`。

## 目录结构

共享 home 模式：

```text
<context>/
├── active -> releases/<version>
├── releases/
├── home/
└── relo.yaml
```

版本独立 home 模式：

```text
<context>/
├── active -> releases/<version>
├── releases/
├── homes/
└── relo.yaml
```

Unix 下的 `active` 是相对目录符号链接；Windows 下则是 NTFS 目录 junction，
因此全局激活不需要开发者模式或管理员终端。Junction 保存绝对目标：移动 Windows
context 后，需要运行 `relo use -g <version>` 重建 `active`。需要使用全局激活的
Windows context 必须位于 NTFS 文件系统。

共享 home 模式下，所有 release 共用 `<context>/home`。版本独立 home 模式下，每个 release 使用 `<context>/homes/<version>`。

release 目录名必须是 `<version>` 或 `<version>_<label>`，其中 `<version>` 是一段或多段的点分数字版本，也可以用 `v` 或 `V` 开头，例如 `8`、`8.1`、`v3.9.9`、`8.1.1.7`、`3.9.9_arm64`、`V1.12.0_darwin-arm64`。版本比较只使用 `_` 前面的数字版本部分，并忽略可选的 `v` 或 `V` 前缀。

## 快速开始

```bash
mkdir -p ~/Documents/30_Toolchain/Maven
cd ~/Documents/30_Toolchain/Maven
relo init
mkdir -p releases/3.8.8 releases/3.9.9
relo use -g 3.9
relo list
```

也可以用下载下来的 `relo` 二进制先初始化一个管理 `relo` 自身的 context，再把这个二进制放进该目录布局里：

```bash
mkdir -p ~/Documents/30_Toolchain/relo
./relo -d ~/Documents/30_Toolchain/relo init
mkdir -p ~/Documents/30_Toolchain/relo/releases/0.1.9/bin
cp ./relo ~/Documents/30_Toolchain/relo/releases/0.1.9/bin/relo
~/Documents/30_Toolchain/relo/releases/0.1.9/bin/relo -d ~/Documents/30_Toolchain/relo use -g 0.1.9
```

之后可以把 `~/Documents/30_Toolchain/relo/active/bin` 加入 shell 的 `PATH`，或者直接从这个 active 路径调用 `relo`。

`relo init` 会创建 `releases/`、home 目录和 `relo.yaml`。如果 `relo.yaml` 已存在，默认不会覆盖；需要覆盖时传 `--force`。

如果省略 `version`，`relo use`、`relo print release`、`relo print path`、`relo print env` 等需要 release 上下文的命令会优先选择 active release；如果还没有 active，则选择 `latest`。

## 命令总览

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
relo [-d <dir>] mac unblock [-v] [version]
relo [-d <dir>] win env apply [version] [--scope user|system] [--path-append] [--yes] [--dry-run]
relo [-d <dir>] win env status [--scope user|system] [--all] [--json]
relo [-d <dir>] win env remove [--scope user|system] [--id <context-id>] [--yes] [--dry-run]
relo [-d <dir>] win env prune [--scope user|system] [--yes] [--dry-run]
relo init zsh
relo init bash
relo init powershell
relo init cmd
```

`-d` 用来选择 context 目录。如果不传 `-d`，`relo` 会优先使用 `RELO_CONTEXT`，其次使用 `RELO_CTX`，都未设置时才使用当前工作目录。这能让一次性脚本更简洁：

```bash
RELO_CONTEXT=/opt/relo/maven relo init
RELO_CTX=/opt/relo/maven relo init
RELO_CTX=/opt/relo/maven relo use -g 3.9.9
```

`-d` 的优先级高于 `RELO_CONTEXT` 和 `RELO_CTX`。

### macOS 隔离属性

macOS 可能会给从浏览器等渠道下载的软件添加 `com.apple.quarantine` 隔离属性。如果你已经确认 release 的来源可信，可以显式移除该属性：

```bash
relo mac unblock 3.9.9
```

省略版本时，与其他需要 release 上下文的命令相同：优先选择 active release，否则选择 latest。该命令只递归移除选中 release 的隔离属性，不修改文件内容、执行权限、代码签名或系统 Gatekeeper 设置。它不会在 `init` 或 `use` 时自动执行。

传入 `-v` 或 `--verbose` 会将选中的版本、release 路径、隔离属性名和递归模式输出到 stderr，并在删除前通过 `xattr` 的 verbose 读取模式显示实际带有隔离属性的路径：

```bash
relo mac unblock -v 3.9.9
```

### Windows 持久环境变量

现有 local `relo use` 行为保持不变。Windows 下需要让配置的环境变量和
PATH 对后续进程持久生效时，显式使用独立的 `win env` 命令组：

```powershell
relo use -g 3.9.9
relo win env apply
relo win env status
```

`apply` 省略版本时使用 active release；显式传入版本不会修改 `active`。
默认写入当前用户作用域，system 作用域需要管理员终端：

```powershell
relo win env apply 3.9.9 --scope system --yes
```

多个 context 可以共同贡献 PATH。新 context 默认加入 Windows `Path` 前部；
`--path-append` 将它放在后部。重复 apply 时保持现有 PATH 位置，除非显式传入
`--path-append`。普通环境变量采用“最后 apply 的 context 生效”语义。

所有权协议通过保留的 `RELO_*` 环境变量公开表达。每个 context 使用由大小写
不敏感路径哈希生成的 ID 管理自己的 PATH 和环境变量 provider；
`RELO_CONF_PATH_PREPEND` 与 `RELO_CONF_PATH_APPEND` 按顺序记录 context ID，
`RELO_PATH_PREPEND` 与 `RELO_PATH_APPEND` 保存根据该顺序物化的具体路径，`Path`
直接引用这两个 aggregate。`RELO_OWNER_<NAME>` 记录公共环境变量当前由哪个 context
管理。这样 Path 只需一级展开，不再依赖 context provider 的递归展开。旧版引用型
aggregate 状态会在下一次写操作时自动迁移。因此 `RELO_` 是保留前缀，不能用于
`relo.yaml` 的持久 env 配置。

首次接管非 relo 环境变量前，`apply` 会展示旧值和新值并要求确认。relo 不保存
被覆盖的原值；当前 winner 或最后一个 provider 被移除时，公共变量会被删除，
不会恢复旧值，也不会自动选择 dormant provider。使用 `--dry-run` 预览，自动化
执行时使用 `--yes`。

`remove` 默认移除当前 context，也可用 `--id` 精确移除已登记 ID。`prune` 清理
记录路径已经不存在的 context。修改只对之后启动的进程生效，apply 后需要重新打开
终端。user 与 system 两个作用域彼此独立。

## 常见工作流

创建 context 并使用共享 home：

```bash
relo -d /opt/relo/maven init
mkdir -p /opt/relo/maven/releases/3.9.9
relo -d /opt/relo/maven use -g 3.9.9
```

创建版本独立 home：

```bash
relo -d /opt/relo/jdk init --home versioned
mkdir -p /opt/relo/jdk/releases/21.0.2_darwin-arm64
relo -d /opt/relo/jdk use -g 21
```

查看当前 context 状态：

```bash
relo show
relo list
relo print active
relo print version
```

查看某个版本最终会导出的路径和环境变量：

```bash
relo print path --version 3.9
relo print env --version 3.9
```

## Shell 使用方式

临时使用某个版本时，`relo use <version>` 会输出 shell 脚本。POSIX shell 中通常这样使用：

```bash
eval "$(relo use 3.8)"
```

安装 shell wrapper 后，可以让 `relo use <version>` 直接影响当前 shell：

```bash
eval "$(relo init zsh)"
```

在 `.zshrc` 中加入同一行即可持久化；bash 用户使用 `relo init bash`。

Windows 下，临时使用默认输出 PowerShell 脚本：

```powershell
Invoke-Expression (relo use 3.8)
```

安装 PowerShell wrapper，让 `relo use <version>` 直接影响当前 PowerShell 会话：

```powershell
Invoke-Expression (relo init powershell)
```

默认 shell 输出按平台自动选择：Windows 使用 PowerShell，Linux 和 macOS 使用 POSIX shell。也可以显式指定：

```bash
relo use --shell posix 3.8
relo use --shell powershell 3.8
relo use --shell cmd 3.8
```

传 `-v` 会把选中的版本、release 路径、模式、最终 env 和最终 path 条目打印到 stderr，不影响 stdout 输出的 shell 脚本：

```bash
eval "$(relo use -v 3.9)"
```

## PATH 处理

`relo.yaml` 控制哪些目录加入 `PATH`：

```yaml
path:
  - ${relo.active}/bin
```

默认情况下，path 条目会放在现有 `PATH` 前面，确保当前选择的 release 优先。`--path-append` 改为放在现有 `PATH` 后面。`--path` 用于 local use 的临时路径：

```bash
relo init --path '${relo.release}/bin' --path tools/bin
relo use 3.9 --path '${relo.release}/sbin' --path /opt/tools/bin
relo use 3.9 --path-append
```

`--path` 只对 local use 有效，不能和 `relo use -g` 一起使用。最终 path 顺序是：临时 `--path`、release 专属 `path`、全局 `path`。

`print path` 每行输出一个解析后的路径，`print env` 每行输出一个 `KEY=value`：

```bash
relo print path --version 3.9
relo print env --version 3.9
```

macOS/Linux 的 POSIX shell 中，`PATH` 使用 `:` 分隔。`paste -sd: -` 会将逐行路径合并为一行以 `:` 分隔的字符串：

```bash
export PATH="$(relo print path | paste -sd: -):$PATH"
```

PowerShell 使用 `;` 作为路径分隔符：

```powershell
$env:PATH = ((relo print path) -join ';') + ';' + $env:PATH
```

对于不包含空格或 shell 特殊字符的简单环境变量值，可以用 `xargs` 把 `KEY=value` 传给 `export`：

```bash
export $(relo print env | xargs)
```

如果值里可能包含空格，使用保留整行的循环更稳妥：

```bash
while IFS= read -r entry; do export "$entry"; done < <(relo print env)
```

相对路径相对于 relo context 解析。绝对路径保持原样。当路径应指向选中的 release、home、active 托管链接或 context 目录时，使用变量：

```text
${relo.release}/bin  -> 选中的 release/bin
${relo.home}/bin     -> 选中的 home/bin
${relo.active}/bin   -> active 托管链接/bin
${relo.context}/bin  -> context/bin
${relo.ctx}/bin      -> context/bin
tools/bin            -> context/tools/bin
/opt/bin             -> /opt/bin
```

包含 `..` 的相对路径会被拒绝。

## 版本解析

支持的版本表达式：

```text
latest
3
3.5
3.5.0
8.1.1.7
8.1.1.10.2
3.5.0_arm64
```

规则：

- `latest` 选择点分数字版本最高的 release。
- `3`、`3.5`、`8.1.1.7` 这类数字表达式可以包含任意段数；如果没有精确匹配完整目录名，则作为前缀表达式选择匹配范围内最高的点分数字版本。
- `3.5.0` 会优先匹配无 label 的 `3.5.0` 目录。
- `3.5.0_arm64` 这类完整目录名会精确匹配。
- 如果只存在多个带 label 的同版本目录，例如 `3.9.9_arm64` 和 `3.9.9_internal`，表达式 `3.9.9` 是歧义的，必须指定完整 release 名称。

## 配置

每个 context 使用自己的 `relo.yaml`：

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

`relo init` 写出的配置会省略默认值；读取时仍会合并默认配置。默认 `path` 是 `${relo.active}/bin`，默认 `home_mode` 是 `shared`，默认 `version_separator` 是 `_`。

`env` 的值都是字符串。`env` 和 `path` 的值支持变量展开：

```text
${relo.context}    relo context 目录
${relo.ctx}        relo context 目录
${relo.active}     active 托管链接路径
${relo.release}    选中的 release 目录
${relo.home}       选中的 home 目录
${relo.version}    选中的 release id
${env.NAME}        之前展开过的已配置 env 值
${sys.NAME}        继承的系统环境变量
```

`env` 按顺序展开。先展开全局条目，再展开 release 专属条目。后面的 env 条目可以通过 `${env.NAME}` 引用前面的条目。Release 专属的 `env` 值会覆盖同名的全局值，影响后续展开和最终输出。

`${relo.root}` 作为 `${relo.context}` 的兼容别名保留，但新配置应使用 `${relo.context}` 或 `${relo.ctx}`。

`${relo.active}` 表示 active 托管链接路径本身，例如 `<context>/active`。Unix 使用符号链接，Windows 使用目录 junction。它不是解析后的 release 目录；选中 release 的真实目录应使用 `${relo.release}`。

Release 专属的 `path` 条目排在全局 path 条目之前。`path` 在 env 展开之后展开，因此可以引用最终的 `${env.NAME}` 值。路径条目可以是绝对路径或相对路径；相对路径在 relo context 下解析。开头的 `~` 会展开为当前用户的主目录，env 和 path 值中都支持。Env 值不会被视为路径处理。Local `relo use` 只导出配置中的 `env` 值和 `PATH`，不会隐式注入 relo 内部变量。`relo use -g` 只更新 `active`；临时 `--path` 覆盖仅在 local use 时有效。

## 命令说明

`init` 初始化 context。默认使用共享 home；传 `--home versioned` 可改为版本独立 home；传多个 `--path` 可设置初始 path 配置。

`list` 列出 `releases/` 下的 release。当前 active release 前面显示 `*`，无效 release 目录前面显示 `!`。

`show` 不带版本时显示 context、active、release、home、home mode 和 release 数量；带版本时显示该版本的 release 路径、home 路径和是否 active。

`use -g` 更新 `active` 托管链接，不输出 shell export。Unix 使用符号链接，Windows 使用目录 junction。`use` 不带 `-g` 时输出当前 shell 可执行的环境变量脚本，并在需要时创建对应 home 目录。

`print` 面向脚本使用，只输出单一目标内容。`context`、`ctx` 和 `active` 不接受 `--version`；`release`、`home`、`version`、`path`、`env` 可以通过 `--version` 指定版本。

`config show` 输出当前 context 的 `relo.yaml` 原文。

## 排错

`not a relo context`：当前目录、`RELO_CONTEXT`、`RELO_CTX` 或 `-d` 指向的目录里没有 `relo.yaml`。

`missing releases directory`：context 里缺少 `releases/` 目录。通常应重新运行 `relo init` 或手动补齐目录。

`active exists but is not a managed link`：`active` 已存在但不是 relo 管理的符号链接或 junction。为了避免覆盖用户文件，`relo` 会拒绝继续。

`active points to missing release`：`active` 指向的 release 目录不存在。需要恢复该 release，或用 `relo use -g <version>` 切换到存在的 release。

`invalid release directory`：`releases/` 下的目录名不是 `<version>` 或 `<version>_<label>`。

`ambiguous release`：版本表达式匹配到多个带 label 的同版本 release。请使用完整目录名。

`path overrides are only valid for local use`：`--path` 只能用于 local use，不能和 `-g` 一起使用。

## 设计边界

`relo` 第一版只做 release 布局定位和切换，不实现：

- install
- download
- upgrade
- remove
- scan
- plugin
- shim
- project local version

如果需要安装或升级软件，建议用已有包管理器、内部发布系统或脚本把 release 放入 `releases/<version>`，再交给 `relo` 做版本解析、active 切换和 shell 环境输出。
