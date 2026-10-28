# 官方 SIWC 实施计划

审查日期：2026-10-27（America/Los_Angeles）。基础提交：`a08de6a`；工作分支：`feat/official-chatgpt-oauth`。

## 官方依据

开发前读取了以下官方页面及 OAuth discovery：

- [概述](https://developers.openai.com/siwc/token-sharing-open-source)
- [注册与登录](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [账号与会话](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
- [模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
- [错误与恢复](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)
- [Token 参考](https://developers.openai.com/siwc/token-sharing-open-source/token-reference)
- [OIDC discovery](https://auth.openai.com/.well-known/openid-configuration)

公开授权端点是 `/api/accounts/authorize`，Token Endpoint 是 `/api/accounts/oauth/token`。Discovery 当前声明 RS256、官方 JWKS 与撤销端点。模型目录使用 `models` 数组及 `slug`、`display_name`、`visibility`。

## 仓库审查与复用边界

| 模块 | 结论 |
| --- | --- |
| `codex/auth` | 固定的其他客户端 ID、旧回调地址、未验证签名的 JWT 解析和进程内刷新锁不满足新协议；独立实现。 |
| `codex/client.rs` | 私有路由、Originator、内部 beta 请求头、WS/HTTP 切换不能复用。 |
| `codex/translate/request.rs` | 复用不读取 Codex 配置的转换入口，再做 SIWC 输入校验、字段白名单及 additional_tools 转换。 |
| `codex/translate/live_stream.rs`、`accumulate.rs` | 复用文本、工具、usage 和 Anthropic SSE 转换；外层严格要求 response.completed，拒绝中断和伪成功。为共享转换器增加可关闭的 Read 停滞修复开关，SIWC 关闭提前结束行为，上游默认不变。 |
| `codex/events.rs` | 旧错误分类存在私有接口语义；SIWC 保留自身 HTTP 状态、结构化错误码和请求 ID。 |
| `codex/mod.rs`、`native.rs`、`continuation.rs`、`compaction.rs`、`websocket.rs` | 不复用其认证、推理、续接或服务端压缩流程。完整历史由 Claude Code 提供，摘要由 Claude Code 发起普通请求。通用 server 仍保留清理内存 compaction 标记的逻辑，不会为此发起请求或读写历史文件。 |
| `registry.rs`、`server.rs`、`config.rs`、`main.rs` | 加入独立 SIWC 命令、配置、动态模型列表和隔离 registry；禁用旧可选原生端点与流量捕获。 |
| `auth.rs`、`paths.rs` | 只沿用应用配置目录约定；新凭据不读取 Codex/Claude 存储。 |
| `session.rs`、监控与其他 provider | 保留现有行为，仅增加 SIWC 路由标识；会话历史不由代理存储。 |
| 现有 `tests/` | 作为回归基线；新增本地 HTTP OAuth/JWKS/API mock 测试。 |

## 实施架构

新增 `src/providers/chatgpt_siwc/`：

1. `auth`：动态注册、稳定 host ID、PKCE/state/nonce、loopback 回调、RS256/JWKS 验证、重新授权、撤销、刷新。
2. `store`：独立的带标签账号记录；Unix 0700 目录、0600 文件；原子替换及 OS 文件锁；刷新前重读，持久化 refresh-in-flight 标志，崩溃后要求重新授权，避免再次使用可能已旋转的 Token。
3. `client`：固定公开 API 地址、Bearer、真实应用 User-Agent；关闭重定向；请求和空闲超时；只对明确的临时 HTTP 失败有限退避，已开始的流不重放。
4. `translate`：验证工具 ID/结果关联和能力限制；复用消息转换；只生成允许字段；函数工具通过 developer additional_tools 声明。
5. `stream`：有界 SSE 帧解码；保留错误；只把 response.completed 认定为成功。
6. `mod`/`cli`：Provider 与命令入口；`siwc serve` 只服务当前显式选择账号，127.0.0.1 监听；可用模型来自官方接口。

唯一计划新增生产依赖为维护中的 `jsonwebtoken`（RS256 验证及 JWK 解析）；文件锁使用当前 Rust 标准库。文件凭据先支持 macOS/Linux，其他平台不以弱权限方式降级。保留 MIT 许可和版权。

## 技术风险与处理

- 授权成功不等于获得套餐权限；检查 Token 响应 scope，缺少权限时保留身份但停止推理。
- 不因 401/403/429 切换账号或计费路径；刷新终止错误清除 Token 并保留注册映射。
- OAuth 网络结果不明确、刷新中崩溃或落盘失败可能要求用户重新授权；宁可停止，也不重用不确定的 Refresh Token。
- 不使用服务端持久会话、私有压缩或完整 Codex harness；`/resume` 依赖 Claude Code 重发历史。
- 某些 Anthropic 内容、hosted tools、动态工具搜索和旧 provider reasoning 签名可能无法等价映射；拒绝或明确记录降级，不能静默丢失工具结果。
- 本地 token counting 为估算，实际上下文窗口和权限由上游决定。
- Mock 验证协议与路由；真实 OAuth、真实模型推理及 Claude Code UI/工具验收需要用户浏览器操作，单独标记。

## 验收标准

- OAuth：随机性、state/nonce/签名/issuer/aud/exp/身份检查、issued client ID、scope、账号隔离、轮换锁、撤销、重启。
- API：完整 input、参数白名单、additional_tools、模型发现、错误码/状态/request ID、失败流与断流、有限重试、无重定向或私有后端回退。
- 工具：调用/结果 ID 一致、多轮文本及读写/Bash/MCP 函数保留、异常显式反馈、重启重放一致。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`；macOS arm64 构建。
- 四份要求文档、可执行人工验证步骤、修改清单及本地 Git Commit；不推送、不创建 PR。
