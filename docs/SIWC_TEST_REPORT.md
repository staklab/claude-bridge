# SIWC 测试与交付报告

首次交付日期：2026-10-27；最新验收：2026-10-28（America/Los_Angeles）。基础提交：`a08de6a`；分支：`feat/official-chatgpt-oauth`。交付提交通过 `git log -1 --oneline` 查看。代码只在本地提交，未推送远端、未创建 PR。

首次交付提交为 `2f5fe50`。本报告保留首次交付记录，最新的响应处理修复和真实复测见最后一节。

## 首次交付验收状态（2f5fe50）

| 层级 | 结果 |
| --- | --- |
| SIWC 自动测试 | 已通过 34 项模块测试，覆盖真实本地 HTTP mock、签名验证和双进程刷新竞争。 |
| 全仓库回归、格式与 Clippy | 全部通过：1174 项测试，0 失败、0 忽略；fmt 与 Clippy 无错误。 |
| macOS Apple Silicon Release | 构建通过，确认是 Mach-O arm64；独立临时目录中的 CLI 冒烟通过。 |
| 真实 OAuth | **待人工验证**。没有操作用户真实授权，也没有读取其密码或 Token。 |
| 真实账号模型与套餐额度推理 | **待人工验证**。Mock 模型目录和 SSE 不证明任何真实账号的资格。 |
| Claude Code 实际文件/Bash/MCP/Skills/Hooks/Subagents | **待人工验证**。自动测试仅验证转换和协议关联，不运行真实工具。 |
| Claude Code `/compact`、原项目 `/resume` | **待人工验证**。自动测试验证历史重放和实例重建，不代表实际客户端长会话验收完成。 |

## 首次交付环境与执行结果

- macOS 27.2，Apple Silicon `arm64`。
- `rustc 1.95.0 (59807616e 2026-04-14)`，host `aarch64-apple-darwin`。
- 基线 `cargo test --locked`：**1139 passed / 0 failed / 0 ignored**。
- 新增模块 `cargo test chatgpt_siwc --lib`：**34 passed / 0 failed / 0 ignored**；包括双进程测试使用的子进程入口。
- 最终 `cargo test`：**1174 passed / 0 failed / 0 ignored**，比基线增加 34 项 SIWC 模块测试和 1 项 CLI 集成测试。其中文库 980、二进制 6、集成测试 188；文档测试 0。
- 生产新增依赖：`jsonwebtoken 11.1.0`，使用 `aws_lc_rs` 验证 RS256。`Cargo.lock` 固定实际解析版本；未进行独立第三方安全审计。

| 命令 | 最终结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过，无警告 |
| `cargo test` | 通过：1174 passed / 0 failed / 0 ignored |
| `cargo build --release --locked` | 通过，optimized Release，构建耗时 1m 38s |
| Release CLI、文件架构及权限检查 | 通过：版本/help、空账号 status、logout 保留 Host ID、0700/0600、未登录的 serve/models 拒绝 |
| `git diff --check` | 通过 |

构建和验证不需要付费 API Key。自动测试使用本地服务、临时私有目录及公开测试 RSA fixture；测试密钥不是任何生产账号凭据。

本地二进制：`target/release/claude-code-proxy`（`claude-code-proxy 0.1.47`）。`file` 输出为 `Mach-O 64-bit executable arm64`。本次构建 SHA-256：`a19131ee1451607b4af3056868f19cf053ec5758d6822bfb2c502331dfcdebd8`。二进制位于 Git 忽略的构建目录，源码提交不包含编译产物。

## 首次交付自动测试覆盖

| 类别 | 已验证行为 |
| --- | --- |
| 动态授权 | 初次 dynamic_agent_client、真实应用名称、稳定 UUIDv4 Host ID、独立 state/nonce/verifier、PKCE S256、正式 issued ID、返回登录不再发送 agent_name_hint。 |
| 回调 | 真实回环 HTTP listener、正确路径、错误 state 拒绝、授权取消不换 Token、返回 client_id 与选中账号一致。 |
| ID Token | 本地 RSA/JWKS 签名验证；拒绝错误签名、unknown kid、nonce、issuer、audience、azp、过期、未来 iat 和空 subject；返回账号身份不符时保留原记录。 |
| Scope | 缺少套餐 scope 时保存身份并停止推理；刷新后 scope 被移除同样停止。 |
| 凭据 | 0700 目录、0600 文件、原子写入、拒绝 symlink；重启重读；同邮箱不同注册不覆盖；注销保留 Client ID 和 Host ID。 |
| 刷新 | 协程并发以及两个独立 OS 进程同时刷新只发生一次轮换；请求省略 scope；替换 access/refresh Token；失效/重用错误清除 Token；临时失败保留；不确定状态禁止重用；尊重 earliest_refresh_at。 |
| 撤销 | 官方 discovery/revoke 流程；远端失败仍本地注销并报告未确认；注销后活动流停止。 |
| 模型与路由 | 读取 models/slug/display_name/visibility；只向公开形状的 `/v1/models` 和 `/v1/responses` mock 路径请求；Bearer 与应用 User-Agent；不发送 Originator 或内部 beta 头。 |
| 请求体 | store=false、stream=true、完整 input、developer/instructions、additional_tools；过滤 Anthropic 不兼容参数；拒绝禁止的 Responses 字段、hosted tools、无法转换的内容和错误工具历史。 |
| 工具 | Read/Write/Bash/MCP 函数定义、并行调用/结果、is_error 回传、调用 ID/名称/参数一致性；无关联或变化的终止工具调用报错。 |
| SSE | UTF-8 字节分片、CRLF、多行 data、真实增量文本早于终止事件到达；文本/工具转 Anthropic 事件；断流、无完成事件、failed/incomplete 均不产生 message_stop。 |
| 延后错误 | Read 参数异常停滞后才到达的额度错误仍被保留；不会因共享转换器的旧修复逻辑提前宣布成功。顶层 error 的 code/param/request ID 可传到客户端。 |
| 错误恢复 | 401/403/404/429 状态与 JSON 形状保留；模型不可用及额度错误明确返回；429 不重试、套餐限额暂停新推理；503 最多三次请求；重定向不携带凭据跟随。 |
| 进程隔离 | SIWC registry 不含 Codex provider；即使配置打开旧原生功能，Responses/Chat Completions/images/transcriptions 入站路由仍不挂载；未知模型不会改走旧后端。 |
| 本地访问 | 拒绝浏览器 Origin、非回环 Host、非回环监听；CLI 使用独立存储，未登录无法启动 siwc serve。 |
| 历史恢复 | 销毁并重建 provider/router 后，同一客户端历史生成相同完整 input。真实 Claude Code 历史文件不在测试范围。 |

## 人工验收入口

按 [使用指南](SIWC_USAGE.md) 顺序执行：用户浏览器授权 → status → models → curl 文本 → 无敏感测试项目的文件/Bash/多轮/MCP → `/compact` → 重启代理后 `/resume` → 最后才恢复原项目。

真实账号授权必须由用户本人完成。不得把 `HTTP 200`、模型目录可见或 mock 通过当作完整推理成功；流中必须出现 `response.completed` 对应的成功结束。发现错误时保留客户端历史，人工判断是否重试，以免重复工具副作用。

## 已知限制

- Unix 私有文件存储，未接 Keychain；Windows 凭据 ACL 和跨平台构建尚未实现/验证。
- hosted tools、动态 tool_search、文件/PDF 内容和原生 Responses 入站未实现；遇到这些请求明确报错。详见 [兼容性表](SIWC_COMPATIBILITY.md)。
- max_tokens/temperature/stop_sequences 等客户端字段没有等价上游控制；token counting 为估算。
- 旧 provider 的隐藏思考签名不重放；完整文本/工具历史与客户端摘要仍由 Claude Code 提供。
- 模型权限、当前套餐额度、长上下文上限、真实工具行为及 Claude Code 版本差异尚需线上验收。
- 使用官方授权不代表零账号风险；服务端策略、工作区授权与接口预览变化仍可能阻止访问。

## 首次交付修改文件清单

| 文件 | 作用 |
| --- | --- |
| `Cargo.toml`、`Cargo.lock` | JWT/JWKS 验证依赖及锁定版本。 |
| `src/providers/chatgpt_siwc/auth.rs` | OAuth、回调、ID Token、刷新与撤销。 |
| `src/providers/chatgpt_siwc/store.rs` | 账号隔离、私有存储与跨进程锁。 |
| `src/providers/chatgpt_siwc/client.rs` | 官方 API、模型目录、错误与有限重试。 |
| `src/providers/chatgpt_siwc/translate.rs` | 参数白名单、完整历史与函数工具。 |
| `src/providers/chatgpt_siwc/stream.rs` | SSE 完成与工具完整性检查、错误传播。 |
| `src/providers/chatgpt_siwc/mod.rs`、`cli.rs` | Provider 与 siwc 命令。 |
| `src/providers/chatgpt_siwc/tests.rs` | 34 项自动测试。 |
| `tests/fixtures/siwc/README.md`、`jwks.json`、`test-rsa.der` | 公开的本地签名测试 fixture。 |
| `src/providers/mod.rs`、`src/provider.rs` | 注册模块与动态模型目录接口。 |
| `src/config.rs`、`src/main.rs`、`src/registry.rs` | CLI、默认模型与独立路由。 |
| `src/server.rs`、`src/session.rs`、`src/traffic.rs` | 回环保护、动态目录、错误监控、会话标识与禁用流量捕获。 |
| `src/providers/codex/translate/live_stream.rs` | 增加 Read 停滞修复开关；SIWC 关闭提前结束，上游默认行为保留。 |
| `src/providers/kimi/translate/request.rs`、`src/tui.rs` | Rust 1.95 Clippy 的三处既有 collapsible_match 警告，做等价 match guard 调整以通过要求的检查。 |
| `tests/cli.rs` | SIWC 命令、独立目录与监听限制集成测试。 |
| `README.md` | 官方 SIWC 入口与文档链接。 |
| `docs/SIWC_IMPLEMENTATION_PLAN.md`、`SIWC_USAGE.md`、`SIWC_COMPATIBILITY.md`、`SIWC_TEST_REPORT.md` | 架构、运行、验收、安全限制及本报告。 |

原 MIT `LICENSE` 和版权声明保留。未读取、修改、删除或重写用户 `~/.claude/projects/`。回滚步骤见 [使用指南](SIWC_USAGE.md#7-退出与回滚)。

## 响应头修复与真实复测（2026-10-27）

修复基于 `2f5fe50`，沿用现有 SIWC 模块、OAuth 和凭据格式。

### 根因及证据

复用用户已授权、`plan_permission=true` 的 default profile，使用其当前有效的 Access Token，向 `https://api.openai.com/v1/models` 和 `https://api.openai.com/v1/responses` 发送最小请求。模型取自官方可见目录，实测 slug 为 `gpt-6.1-sol`。推理请求使用完整的本次 input、`store=false`、`stream=true`，未包含禁止参数。

当前网络链路两次返回 HTTP 200，正文包含合法 Responses SSE 和 `response.completed`，但没有 Content-Type；请求 ID 头名为 `x-oai-request-id`。现有环境配置了 HTTP/HTTPS 代理，仅凭客户端证据无法确定是源站还是中间链路省略该头。

旧代码已能接受 charset 参数，非 2xx 也已在传输层先处理。此次直接失败来自 Reader 把缺少 Content-Type 的有效 SSE 一律拒绝。此外，旧错误丢失响应元数据，并将本地 502 写成 `upstream_status`，影响定位。

### 修改

- `src/providers/chatgpt_siwc/client.rs`：保留真实 HTTP 元数据、兼容三种请求 ID 头；使用 `upstream_status`/`proxy_status` 区分上游与本地状态；诊断日志仅输出允许字段；脱敏凭据及请求文本回显。
- `src/providers/chatgpt_siwc/stream.rs`：状态检查优先；接受合法媒体类型参数；缺失 Content-Type 时进行有界 SSE 首帧探测；200 JSON/HTML 显式报协议错误；继续要求 `response.completed`，将请求 ID 和元数据保留至流内错误。
- `src/providers/chatgpt_siwc/tests.rs`：新增 6 项回归，覆盖 400/401/403/429 JSON 与异常 Content-Type、普通/charset/缺头 SSE、200 JSON、真实传输中断、缺少完成事件、探测大小上限和诊断脱敏。
- `README.md`、`docs/SIWC_USAGE.md`、`docs/SIWC_COMPATIBILITY.md`、本报告：记录诊断字段、缺头处理和验收证据。

OAuth、凭据存储、端点和请求转换器未修改。凭据文件校验确认保持原样；没有重新登录、注销或调用私有 Codex 推理端点。

### 最新验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo test chatgpt_siwc --lib` | 40 passed / 0 failed |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test` | 1180 passed / 0 failed / 0 ignored（15 组测试；其中 SIWC 模块 40 项） |
| `cargo build --release --locked` | 通过，Mach-O arm64 Release |
| 新 Release 本地 `/v1/messages` → 真实公开 API | 通过，现有 default profile / gpt-6.1-sol；流式和非流式都返回 HTTP 200、`SIWC OK`，并确认推理完成 |

最终 Release SHA-256：`9cfdc33d5f0faae70f91650728cca3e6e5b1108d1a1b23659b596011b37f00a7`，与真实复测使用的二进制一致。

真实复测使用临时回环端口和最小文本，不发送项目文件或工具。两次实际上游都为 HTTP 200、Content-Type 缺省；流式转发以 `message_stop` 结束，非流式在上游完成后返回完整 Anthropic JSON。请求 ID 分别为 `req_df9f4ba9790f4a839a00a5dc3649afcf` 和 `req_fafd4d806b45416dae84b9d68df89964`。复测前后凭据文件逐字节摘要相同；测试启动的进程已退出。

真实 OAuth 为用户已完成的授权，本次复用验证；没有重做浏览器登录。Claude Code 实际 Read/Write/Bash/MCP、长会话压缩和 `/resume` 仍待人工验证。单次文本请求完成也不能保证全部模型或工具能力可用。

本次核对的最新官方文档：[模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)、[错误与恢复](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)、[预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)。

## 原生 Claude Code 兼容修复与验收（2026-10-28）

本节记录基于 `fc6441f` 的后续修复；上面的测试数量、二进制摘要和待验项目均为各次交付时的历史记录。

### 真实复现及根因

使用已安装的原生 Claude Code **2.1.310**，在临时空项目和独立 Claude 配置目录中运行 `claude -p`，复用现有 SIWC default profile 和 `gpt-6.1-sol`。没有加载用户项目文件、Hooks、MCP 或历史。

- 普通原生文本请求包含 12 个 custom 工具、system 文本块及一条 system-role 消息，已能返回 `CLAUDE GPT OK`。
- 增加无敏感内容的 system 长度后，原生请求为 138,207 字节。修复前在收到 65,554 字节时命中旧探测的 64 KiB 上限，返回 HTTP 502；实际上游 HTTP 200、Content-Type 缺失，请求 ID 为 `req_564030734a0f4f07bc40e14960ba5a14`。
- 修改后，同样请求完成真实推理。安全诊断确认首事件为 `response.created`，完整首帧 **130,366 字节**，其中包含上游回显的 instructions 和工具定义。旧上限小于合法首事件，是本次复现的直接根因。旧 10 秒首帧期限也过短，本次改为 90 秒并增加延迟首帧回归；本次大小复现约 4 秒即失败，并非超时。
- 用户提供的两个历史 Request ID 没有配套正文或长度记录，无法事后从本地诊断还原其首帧大小；没有将新复现冒充为对这两个历史响应的逐字节检查。

### 请求结构对比

| 官方 Responses 字段 | 最小 curl | 实际原生 Claude Code 转换后 |
| --- | --- | --- |
| model | 官方可见 slug | 同为 `gpt-6.1-sol` |
| store / stream | false / true | false / true；入站非流式也向官方使用 true |
| input | 完整用户消息 | 完整客户端历史；system-role 消息转 developer；函数定义作为 developer additional_tools |
| instructions | 无 | 顶层 system 文本块合并；大请求实测 97,041 字节 |
| tools | 无 | 12 个本地 custom 工具映射为 function，位于 additional_tools；不生成顶层 tools 或 hosted tool_search |
| tool_choice | 无 | 实际文本请求省略；Mock 验证 auto/none/any/具名工具转换 |
| 其他字段 | parallel_tool_calls | 相同；不透传 max_tokens、metadata、thinking、context_management 等 Anthropic 字段 |

转换后的大请求为 138,816 字节、3 个 input 项、12 个函数工具，`system_items=0`、`top_level_tools=false`。没有通过移除 Claude Code 核心工具或丢弃 system 来规避故障。原生工具名 ToolSearch 在此是普通本地函数；代理没有生成官方不支持的 Responses `type: tool_search`。

### 实现与重试行为

- `stream.rs`：按完整 SSE 首事件验证，大小上限与普通帧的 **8 MiB** 一致，首帧期限 **90 秒**；按行增量解码，保留当前 HTTP 分块中首事件之后的字节。正常/charset/缺头 SSE 均支持，BOM、CRLF 和 UTF-8 分片有回归；JSON/HTML/空响应仍失败，只有 `response.completed` 才成功。
- `client.rs`：状态优先；HTTP 200 JSON error 保留真实 code/param 并按已知代码分类，未知错误码也保留。增加 `response_class`、`first_event_type`（固定安全类别）和 `probe_bytes`。诊断不记录正文、提示词、Authorization、工具名称或参数。
- `translate.rs`：增加固定类别、数量和字节数的 `siwc_request_shape` 诊断，不改变现有请求白名单和工具转换。
- `client.rs` / `stream.rs`：推理 POST 不自动重放；错误和流响应添加 `x-should-retry: false`。真实测试发现 Claude Code 遇到流内 `server_is_overloaded` 后会改为非流式重发，单靠响应头无法阻止。现在同一转换后请求失败后，在当前进程内缓存安全错误 **60 秒、最多 16 项**，拦截回退并返回原错误及 Request ID；仅使用不落盘、不输出的 SHA-256 索引。改变请求后可继续。缓存不覆盖并发在途请求、其他进程或时限/容量之外的请求，不承诺恰好一次工具执行。
- `tests.rs` 和脱敏原生 fixture：新增 7 项测试，包含真实原生请求形状、大于 64 KiB 的 response.created、多种 HTTP 分片、同块剩余事件、11 秒延迟首帧、异常 200 分类、诊断脱敏、流式到非流式回退拦截；保留既有 400/401/403/429、断流、缺少完成事件测试。

原生请求 fixture 为 `tests/fixtures/siwc/claude-code-2.1.310.json`，来源与脱敏说明在同目录 README；根目录 README、使用指南及兼容性表同步更新了实测范围和重试行为。

本次修改没有改变 OAuth、Client ID、Host ID、凭据格式或官方端点。授权正常刷新按原流程进行；没有重新注册、注销、删除账号或调用私有推理端点。未修改用户原有 Claude Code 项目或历史。

### 最终验证

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| SIWC 模块测试 | 47 passed / 0 failed |
| `cargo test` | 1187 passed / 0 failed / 0 ignored，15 组测试 |
| `cargo build --release --locked` | 通过，Mach-O arm64 Release |
| 最终 Release 原生 Claude Code 文本 | 通过：大 system / 12 工具的请求返回 CLAUDE GPT OK，退出码 0，上游仅一次推理 |
| 最终 Release 原生 Read / Write / Bash | 通过：三个实际工具调用、四轮推理均完成，工具结果无 is_error；输出文件逐字匹配输入，Bash 返回 BASH VERIFIED，最终为 CLAUDE TOOLS OK，退出码 0 |

最终二进制 SHA-256：`7a7839114332e0a72363d474635a5e7c0d0d910414e2af3a0bef66d802816e75`；最终完整检查后摘要与实机验证使用的文件相同。

最终文本 Request ID 为 `req_e942eac71ce448978824f041c7d83ce5`，首事件为 response.created、130,366 字节。工具链四轮 ID 依次为 `req_e747598baa3a431bbb3bfc34d3eb96b5`、`req_2ed0e91d94624ac19600fa4e9e0b55b2`、`req_b425e1b8014444eaaf61cd9c86eaa67a`、`req_64fbc213de384f08a43d6f7ba6dac1d5`。各轮实际上游均为 HTTP 200、Content-Type 缺失；对应函数调用和结果累计数为 0、1、2、3，所有轮次均完成，没有失败回退或重复转发。最终答复分别严格匹配上述文本。

测试过程中原 Access Token 到期，由现有 OAuth 刷新流程正常续期，因此凭据文件摘要发生预期变化。刷新前后 Client ID 和 Host ID 摘要相同、账号凭据仍存在；没有重新注册或删除账号。每次验收使用临时回环端口、独立 Claude 配置和 `--no-session-persistence`，自建进程结束后均已退出，用户原有代理进程未被停止。

原生 SDK 本地错误实验（不调用官方 API）：带 `x-should-retry: false` 的 502/503 各产生一次 POST；400 仍观察到两次 POST，说明客户端应用层也可能另外发请求。Mock 回归验证流式失败后的非流式请求命中同一上游请求摘要，官方 mock 计数保持 1，并保留首次错误代码、param、200 上游状态及 Request ID。

完整交互式 UI、真实 MCP/Skills/Hooks/Subagents、长会话 `/compact` 与原项目 `/resume` 仍未验收。临时目录的原生文本和工具验证不等于这些能力全部通过。

官方依据：[预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)、[模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)、[错误与恢复](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)。
