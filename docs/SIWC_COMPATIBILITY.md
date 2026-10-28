# SIWC 兼容性与安全边界

本页描述已实现和 Mock 验证的代理行为。真实 OAuth、账号模型推理、Claude Code UI 与真实编程工具验收均待人工验证，不能视为已经完成端到端验收。

## 能力矩阵

| 能力 | 实现状态与限制 |
| --- | --- |
| Anthropic `/v1/messages` 文本 | 流式翻译；非流式调用也向官方发送 stream=true，完成后本地合并。 |
| 多轮历史、代理重启 | 每次转换客户端提供的完整历史；无服务端 previous_response_id、代理历史数据库。Mock 覆盖实例重建后输入一致性。 |
| Read/Write/Bash | 转为 function call，保留 call_id，结果回传；执行由 Claude Code 完成，代理不会执行 Shell 或文件操作。实际工具交互待验证。 |
| 本地 MCP | 普通函数定义及文本/base64 图片结果可转换；建议 ENABLE_TOOL_SEARCH=false，所有工具定义提前加载。 |
| Skills/Hooks/Subagents | 保持客户端原流程，普通消息/函数经 SIWC 转发；未进行真实客户端验收，不能保证所有新 beta 功能。 |
| `/resume` | 不修改 `~/.claude/projects/`。需要 Claude Code 重发历史；真实历史恢复待人工测试。 |
| `/compact` | 作为普通摘要请求处理；客户端保管摘要与历史。没有私有服务端压缩、WS continuation 或 Codex harness。 |
| `/v1/messages/count_tokens` | 使用已有 o200k tokenizer 估算，包含工具；返回 x-siwc-token-count=estimate，不保证模型实际 token 数或上下文容量。 |
| 模型选择 | 官方 `/v1/models` 实时发现、slug/display_name；未知 slug 交给官方 API 判定，不根据硬编码列表承诺可用。 |
| System/developer 指令 | 顶层 system 转 instructions；消息中的 system/developer 转 developer。 |
| Function tools | developer additional_tools 声明；显式拒绝孤立/重复结果、未完成历史调用及未知上游调用。 |
| Custom raw tools | Anthropic 自定义 JSON-schema 工具映射为 function；未实现自由格式 Responses custom grammar 工具。 |
| 图片 | 用户文本中的 base64/HTTPS 图片与工具返回的 base64 图片；模型是否接受由官方决定。工具返回 URL 图片显式拒绝，避免旧转换器静默省略。 |
| Files/PDF/document | 当前代理尚未映射，明确返回不支持；官方支持部分 file input 不代表本代理已实现。 |
| Hidden thinking | 旧 provider 的签名和隐藏思考不重放；保留可见文本和工具历史。新响应的 reasoning summary 可显示，跨轮依靠已提供文本/工具内容。 |
| Hosted tools | 本次实现不开放 web search、image generation、file search、Code Interpreter、computer use、hosted MCP/connectors 或 tool_search。官方 web search 可能可用，本代理暂未适配；其他列出的 hosted 能力受官方预览限制。 |
| 音频/视频、上传、转录 | 未实现；官方此授权路径也不支持音视频输入、Files upload/transcription API。 |
| Responses 原生入站、Chat Completions、WebSocket | SIWC 进程不挂载这些路由。它接受 Anthropic Messages 并通过官方 HTTP Responses 输出推理请求。 |
| beta 服务端 threads | 客户端要求服务器持久线程时仍明确 capability_rejected，不能取代完整本地历史。 |

复用的上游转换器会清除函数 schema 的 pattern 约束，并沿用其 Read 参数/说明修正逻辑。客户端应继续负责工具输入校验和权限。转换器对巨大 Read offset 的原有修正仍可能改变异常工具参数；真实 Read 验收需要检查。SIWC 专门关闭了 Read 空白停滞时提前结束响应的修复逻辑，始终等待官方完成事件；终止事件中的工具 ID、名称和参数必须与此前完成的调用一致。

## 参数约定

请求采用显式白名单构造：model、input、store、stream、parallel_tool_calls，以及有值时的 instructions、tool_choice、reasoning、text。函数定义位于 input 的 additional_tools 项。不会透传任意 extra 字段。

Anthropic 的 max_tokens、temperature、top_p、top_k、metadata、stop_sequences、cache_control 不转发；忽略它们是本地协议兼容行为，不承诺温度控制、输出 token 上限、stop sequence 或缓存效果。输出格式只通过受支持的 output_config 转换。

直接传入 previous_response_id、background、conversation、max_output_tokens、max_tool_calls、client_metadata、service_tier、原生 input 等不支持字段会收到 400。不要用官方 HTTP 请求的 max_output_tokens 来控制这条路线。

## 安全与错误

- 动态 OAuth 注册使用应用真实名称，固定官方 auth 端点与公开 API；不会读取 Codex Token、借用其他客户端 ID、添加 Originator 或内部 beta 请求头。
- JWKS/RS256、issuer、audience、azp、nonce、exp/nbf/iat、账号 subject 均检查；模型调用还必须具备套餐权限 scope。
- Unix 0700 目录、0600 文件、OS 文件锁、原子 rename/fsync。账号按 profile/issued client ID/已验证 identity 隔离，同邮箱不合并。
- 刷新在跨进程锁内重新读取，先写不确定性标记；并发只旋转一次。网络结果不明时保留凭据并要求重新授权，避免 Token 重用。
- 每个进程固定一个明确选择的账号；不能因额度/权限失败自动更换账号、后端或计费方式。
- SIWC 固定回环监听；检查本地连接、Host、Origin 和 JSON Content-Type。它没有抵御同用户恶意本地进程的能力，不应通过公网隧道或公开反向代理暴露。
- OAuth 与 API 均关闭重定向、验证 TLS；生产端点不提供环境覆盖。Mock 地址只能在测试模块内构造。
- 不保存请求/响应流量；不会把授权 URL 中的 ID Token hint 输出到日志。错误正文交给客户端，代理日志不写入 SIWC 上游错误内容。CLI status 不显示 Token。
- 初次 HTTP 失败保留状态、JSON body 形状、code/param、请求 ID，局部脱敏 bearer 回显；未知非 JSON 错误返回通用说明。SSE 失败保留结构化错误及请求 ID。
- 只有 response.completed 成功；response.failed/incomplete、无完成事件、网络中断和工具关联异常都返回错误。不在流内重放请求。
- HTTP 503 明确临时错误最多两次退避；429 不自动重试，套餐额度错误使该代理进程暂停新推理，用户确认额度恢复后再重启。
- 流式 HTTP 状态在开始后不可改变；错误通过 Anthropic error 事件送达，监控使用单独结果标记，不能仅凭 HTTP 200 判断成功。

官方支持的是符合要求的 OSS/本地应用授权和指定预览 API。这并不保证所有账号、工作区、地区、模型或请求能力可用，也不意味着零账号风险。用户授权、套餐设置、服务政策和后续协议变化仍决定实际访问范围。

官方依据：[预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)、[模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)、[账号与会话](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)、[错误与恢复](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)。
