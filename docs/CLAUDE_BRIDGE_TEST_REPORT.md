# Claude Bridge 0.2.0 验收报告

日期：2026-10-28。开发与实测环境：Apple Silicon / macOS 27.2 / Swift 6.4 / Rust 1.95。测试构建，无 Developer ID Application 证书，无 Apple 公证。

**当前交付状态：已有可安装和本地测试的真实 App、DMG、原生运行组件、完整源码与自动化构建。尚未达到面向普通用户公开分发的最终验收要求。** 不将隔离 HOME、Mock、当前账号复用或 launchd 重载说成全新 Mac、首次新账号授权或真实关机重启。

## 实现与发现

| 位置 | 行为 |
|---|---|
| `src/bin/claude-gpt.rs`、`src/bridge/launcher.rs` | 独立全局入口、CWD 与参数保留、原生会话、进程级连接隔离、私有设置文件 |
| `src/bridge/install.rs` | 实际组件安装、官方 Claude Code 下载、PATH 冲突/备份/去重、卸载保留数据 |
| `src/bridge/service.rs` | 用户 launchd 按需启动、随机端口、HMAC 身份、本地 Bearer、并发启动锁、会话租约、空闲退出 |
| `macos/ClaudeBridge` | 原生向导、菜单栏、ChatGPT 登录、模型选择、真实测试、项目启动、修复、更新和卸载 |
| `scripts/macos`、macOS workflow | 完整包构建、许可证、签名、公证、隔离回归、凭据审计和真实 CLI 验证 |
| 原 SIWC OAuth | 仅首次动态注册的应用名称提示及回调说明适配，保留已验证协议与已有凭据 |

此前原生 Claude Code 502 的真实根因是首个合法 SSE 帧超过旧的 64 KiB 探测上限；已在本轮之前的 `7a867c7` 修复，原生文本与 Read / Write / Bash 已实测。本轮未重写该核心链路，继续使用官方 `api.openai.com/v1/responses`。

产品首次启动检查另发现并修复：状态检测会创建空 SIWC 存储，不能仅以文件存在阻止复用已有本机授权；原生交互模式会对 `ANTHROPIC_API_KEY` 额外要求用户确认，虽然 `-p` 可以直接成功。启动器现使用本地 Bearer 并清空其进程内的 Anthropic API Key。新配置目录的真实交互模式已从主题选择进入原生安全提示，未出现 API Key 设置或 Claude 账号登录选择。原生安全与项目信任提示由用户正常完成。

最终并行验收还暴露了 launchd 边界：`bootout` 读取传入 plist 的 Label，可以卸载同一用户域中另一个 HOME 的同名任务。原实现只核对本地 plist，隔离安装回归因此打断了一次同时运行的 CLI 启动。已增加对实际 loaded job 的 program 与 plist 路径精确核对；不能识别的任务不接管。修复后，保留主用户真实服务运行时，隔离安装/卸载完整通过，再完成真实文本、恢复和工具回归。

GUI 项目启动验证发现 JSON 字符串中的斜线转义不适用于 AppleScript。现改用私有、可执行的 `.command` 文档，通过 NSWorkspace 交给 Terminal 打开，不申请 Apple Events 权限。回归验证带空格、单引号、换行及 shell 特殊字符的项目和启动器路径，以及符号链接拒绝。实际点击项目选择后，系统打开请求未返回错误，后台服务已验证启动；当前计算机自动化工具明确禁止操作 Terminal.app，**终端内交互画面与这一次 GUI 启动后的对话仍待人工确认**，不能将系统接受请求等同于完成交互对话。

## 已进行的真实验证

- 实际构建 `.app` 与 `.dmg`，打开 SwiftUI 向导并检查界面。通过 GUI 安装用户级启动器、确认 shell 修改、验证新的 zsh 能识别命令。
- 将测试 App 放在用户 Applications；实际运行组件位于固定 Application Support 路径，CLI 无需源码目录或 Cargo。
- 使用当前 Mac 原有 SIWC 注册与授权，**没有重新注册、删除凭据或复制刷新 Token**。GUI 实际读取账号权限及其官方模型目录，选择目录中的 `gpt-6.1-sol`，完成轻量推理并显示“配置完成，可以使用”。首次套餐提示实际展示并确认。
- 真实轻量测试安全诊断样例：`upstream_status=200`、`upstream_content_type=null`、`response_class=responses_sse`、`first_event_type=response.created`、Request ID `req_b0c057b313e3496f8a9303487218b0b7`。缺少 Content-Type 的合法 SSE 仍成功解析。
- 退出 App 后，通过已安装的 `claude-gpt` 完成真实文本、`--resume`、`--continue` 以及实际 Read / Write / Bash 调用。工具测试使用随机内容的临时项目，验证写入内容与 Bash 工作目录；没有读取用户项目。原生历史写到隔离测试配置，未迁移或清理用户历史。
- 测试刻意给父环境设置错误 API 地址及 Bedrock 标志，启动器仍使用已验证本地 SIWC 服务。原生 `claude` 文件 SHA-256 在前后相同；没有覆盖它或修改其二进制。
- 对本应用自己的 launchd job 发送 SIGKILL 后，CLI 自动恢复服务并真实回复 `RECOVERY OK`。卸载 job 后，CLI 自动 bootstrap 并真实回复 `BOOTSTRAP OK`。**这些不是重启 Mac 测试。**
- 从没有 `.zshrc`、没有 Claude Code 的隔离 HOME，以 `/usr/bin:/bin:/usr/sbin:/sbin` PATH 运行官方安装器，真实安装并执行 Anthropic 原生稳定版 `2.1.308`。PATH 没有 Rust、Node 或 Homebrew；安装包本身未包含该第三方二进制。主机日常版本为 `2.1.310`。

## 自动验证

最终详细结果与产物摘要另见本目录的 `CLAUDE_BRIDGE_BUILD_RESULTS.json`。

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`：1196 项通过、0 失败；覆盖原有 SIWC JSON 错误、SSE、charset、缺少 Content-Type、大帧、中断和缺少 completed 等测试；新增桌面路径权限、身份伪造、参数与设置隔离、授权存储隔离和租约清理测试。
- `cargo build --release --locked`
- 实际 arm64 包构建、严格本地签名校验及 DMG 校验。
- 包级隔离安装回归：本机 30 项检查（CI 无已加载服务时为 29 项），覆盖空 HOME、纯系统 PATH、zsh 登录/非登录、重复修复、备份、重装、原生 Claude 与历史保留、同名命令冲突、符号链接、`.zshenv` 自定义 ZDOTDIR、包篡改拒绝，以及另一安装的真实 loaded job 不被卸载。
- `bash scripts/macos/test_terminal.sh`：4 项原生 Swift 检查通过，实际执行生成的测试启动文档以验证 CWD 与 shell 引用；这不代替 Terminal.app 的人工界面验收。
- 真实已安装 CLI 回归脚本 `scripts/macos/test_live.py`，需要用户先完成 GUI 授权，会消耗少量套餐用量；不作为无需授权的 Mock 测试运行。
- 安装包逐文件凭据扫描：无账号存储、注册记录或当前开发账号凭据匹配。不会输出匹配原值。第三方许可证收集覆盖 arm64 依赖图，缺失通知会阻止打包。

## 需要独立完成的最终验收

| 验收项 | 当前证据 | 仍需实测 |
|---|---|---|
| 无 Rust / Node / Homebrew | 隔离 HOME、纯系统 PATH 实际安装与运行 | 全新 macOS VM / 独立 Mac |
| 未安装 Claude Code | 隔离 HOME 从官方安装器成功安装原生版 | 全新用户图形向导点选安装 |
| 已有 Claude Code 和大量历史 | 本机复用、二进制不变、隔离历史完整生命周期 | 独立用户大量真实会话的升级/卸载抽查 |
| 从未登录 ChatGPT | 新存储不导入凭据、独立 ID | 独立用户在浏览器首次授权 |
| GUI 首次 OAuth | 复用既有账号通过模型/推理；保留官方浏览器流程 | 新注册的 Claude Bridge 名称、首次授权、回调完整流程 |
| 无手动配置可运行命令 | GUI PATH 验证 + 两种 zsh 自动化 | 三种终端在独立干净账户实测 |
| 退出 App 后 CLI 可用 | 已真实验证 | 全新用户复测 |
| 重启 Mac 后 CLI 可用 | job 卸载后自动 bootstrap 真实验证 | 实际关机/重启，不打开 App 后运行 CLI |
| 原生 claude 不受影响 | 独立入口及二进制前后相同 | 独立用户自己的 Claude 官方账号登录/对话 |
| --resume 历史 | 当前临时项目真实恢复及 continue | 独立用户已有大量历史抽查 |
| 代理异常恢复 | 真实 SIGKILL 后恢复并完成推理 | 全新系统重复验证 |
| 重装/升级/卸载保留数据 | 自动重装/修复/卸载测试保留授权与会话哨兵 | 两个真实版本的升级 + 独立用户历史 |
| Gatekeeper 正常安装 | 本地 ad hoc 严格签名与 DMG 完整性 | Developer ID / 公证 / staple / 隔离下载后的干净 Mac |
| 自动更新 | HTTPS / SHA / 同团队签名 / 安全评估实现 | 发布者托管真实更新源与两个已公证版本 |

本机不能为了验收擅自重启正在工作的 Mac，也没有独立新用户可供本人完成首次浏览器授权。本轮没有分发证书、公证凭据或已授权公开更新站点，未触发远端发布。这些是明确剩余验收条件，不能用 Mock 通过替代。

## 独立全新用户人工验收流程

1. 使用新 Apple Silicon VM / 独立 Mac 和新 macOS 用户；不要迁移开发者 HOME，不安装开发工具。记录系统版本和终端类型。正式验收使用已签名公证且经浏览器下载的包。
2. 按手册用 Finder 安装 App。首次打开只通过界面完成许可、Claude Code 安装和启动器安装；确认没有要求复制配置命令，也未下载 Node / Homebrew。
3. 用户本人点击 Continue with ChatGPT，在官方浏览器页面授权自己的合格账号。确认名称、独立设备注册、套餐提示和 Usage 链接。检查模型仅来自该账号目录。
4. 通过 GUI 选模型并测试，选择临时项目进入原生 Claude Code。完成其必要的安全/项目信任确认，进行一次真实文本对话，再执行 Read、Write、Bash。不要使用 API Key 配置或 Anthropic 登录替代此验证。
5. 在 Terminal、iTerm2、Ghostty 的新 zsh 窗口分别运行 `claude-gpt`。检查 CWD、`-p`、`--resume`、`--continue`，以及默认模型切换不可用时的引导。原生 `claude` 应按用户原有官方认证运行。
6. 退出 App，继续对话。重启 Mac，**不打开 App**，在原项目直接启动 `claude-gpt --resume`。确认服务自动就绪与授权正常刷新；记录真实结果。
7. 在仅有临时测试会话的环境使本应用代理异常退出，确认下一次启动恢复。账号授权失效时检查明确重新授权入口，确认没有其他计费通道回退。
8. 对存在大量历史的测试账户抽取会话摘要/文件校验，依次修复、重装、升级、卸载启动器，比较前后。用两个已公证版本检验自动更新与失败恢复。确认未覆盖其他同名命令、未重复 PATH、未删除用户 OAuth 或 Claude 会话。

测试人员可在已完成 GUI 设置后运行 `python3 scripts/macos/test_live.py` 复现合成项目回归；这是开发/验收工具，Python 不是 App 用户依赖。
