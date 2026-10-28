# 官方 SIWC 使用指南

本分支新增独立的 `siwc` 模式。真实账号授权和 Claude Code 实机验收目前**待人工验证**；Mock 测试不能代表你的账号已获准调用某个模型。当前支持 macOS/Linux 的私有文件凭据，主要构建目标为 macOS Apple Silicon。

## 1. 构建

安装 Xcode Command Line Tools 和 Rust stable（至少 1.89，开发验证使用 1.95）。在本仓库目录执行：

```sh
cargo build --release --locked
export CCP_BIN="$PWD/target/release/claude-code-proxy"
"$CCP_BIN" --version
```

Apple Silicon 原生工具链应为 `aarch64-apple-darwin`，可用 `rustup show active-toolchain` 查看。显式交叉构建使用 `cargo build --release --locked --target aarch64-apple-darwin`，可执行文件相应位于 `target/aarch64-apple-darwin/release/`。

保留已安装的旧版本，先用这个绝对路径运行本分支，便于回滚。

## 2. 用户本人完成授权

```sh
"$CCP_BIN" siwc auth login
"$CCP_BIN" siwc auth status
```

程序先监听 `127.0.0.1` 的可用端口，再打开官方浏览器授权页。用户本人登录、选择工作区并决定是否允许使用 ChatGPT 套餐。不要把密码、回调 URL、授权码或 Token 粘贴到聊天、issue 或日志中。回调页只表示收到结果，最终验证结果以终端为准。

已有注册再次运行相同登录命令，会沿用该账号 Client ID 和设备 Host ID。授权被取消时停止；没有套餐权限时保存已验证身份，推理被禁用。用户决定启用套餐权限后，显式执行：

```sh
"$CCP_BIN" siwc auth login --consent
```

浏览器无法自动打开时：

```sh
"$CCP_BIN" siwc auth login --no-browser
```

该命令只输出不带 ID Token hint 的授权 URL。五分钟内完成授权；Ctrl+C 取消。

多个工作区使用明确的本地标签，每次由用户选择，代理不会自动换账号：

```sh
"$CCP_BIN" siwc auth login --profile work
"$CCP_BIN" siwc auth status --profile work
"$CCP_BIN" siwc models --profile work
"$CCP_BIN" siwc serve --profile work --model '<该账号的模型 slug>'
```

同一邮箱的不同注册独立保存。`--profile` 默认为 `default`；改变 profile 不会自动迁移已有 Token。

## 3. 查询模型并启动

```sh
"$CCP_BIN" siwc models
```

结果中的 `id` 是官方目录的 `slug`，`display_name` 是官方显示名。只显示 `visibility=list` 的条目，并保留服务端顺序。目录显示不代表该次请求一定有资格执行。

设置所选模型，然后启动：

```sh
export CCP_SIWC_MODEL='<siwc models 输出的 id>'
"$CCP_BIN" siwc serve --model "$CCP_SIWC_MODEL" --port 18765
```

也可在已有 `CCP_CONFIG_DIR/config.json`（默认 `~/.config/claude-code-proxy/config.json`）中合并 `{"siwc":{"model":"你的模型 slug"}}`。不要覆盖其他现有配置。优先级是 `--model`、`CCP_SIWC_MODEL`、配置文件；不内置保证可用的默认 GPT 模型。

`siwc serve` 固定监听 `127.0.0.1`，启动时检查登录，运行时重读当前 profile 凭据。这个进程只注册 SIWC 提供方。可选旧 Codex 原生端点、流量捕获、Codex model/Originator/baseUrl/transport 配置不会启用到这条路径。

需要原监控界面时，可显式设置 `CCP_ALIAS_PROVIDER=siwc CCP_SIWC_MODEL=...` 再运行普通 `serve`；这也会创建隔离 registry，但只使用 `default` profile。日常建议使用明确的 `siwc serve` 命令。普通 `serve` 未选择 SIWC 时仍保留上游默认行为。

## 4. 独立终端做文本验证

```sh
curl --fail-with-body --no-buffer http://127.0.0.1:18765/v1/messages \
  -H 'Content-Type: application/json' \
  -d '{"model":"siwc","max_tokens":256,"stream":true,"messages":[{"role":"user","content":"请只回复：SIWC OK"}]}'
```

这里的 `siwc` 别名使用代理配置的默认模型。`max_tokens` 是 Claude Code 请求的兼容字段，不会发送为官方禁止的 `max_output_tokens`，因此不是有效的上游输出上限。

成功应有文本、`message_delta` 和 `message_stop`。若出现 `event: error`，该次请求失败；HTTP 200 只表示流已开始。可用模型也能通过本地 `GET /v1/models` 查询。

## 5. Claude Code 测试与模型选择

在无敏感数据的测试项目中运行。将以下变量中的模型替换成刚查询的真实 slug；也可以在启动代理的同一 shell 中使用已设置的 `CCP_SIWC_MODEL`：

```sh
mkdir -p /tmp/siwc-claude-smoke
cd /tmp/siwc-claude-smoke
ANTHROPIC_BASE_URL=http://127.0.0.1:18765 \
ANTHROPIC_AUTH_TOKEN=unused \
ANTHROPIC_MODEL="$CCP_SIWC_MODEL" \
ANTHROPIC_DEFAULT_SONNET_MODEL="$CCP_SIWC_MODEL" \
ANTHROPIC_DEFAULT_OPUS_MODEL="$CCP_SIWC_MODEL" \
ANTHROPIC_DEFAULT_HAIKU_MODEL="$CCP_SIWC_MODEL" \
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK=1 \
ENABLE_TOOL_SEARCH=false \
  claude
```

在新终端中需重新设置 `CCP_SIWC_MODEL`。这些设置只应用于该次 Claude Code 进程，不写入用户的 Claude 配置或历史。`ANTHROPIC_AUTH_TOKEN=unused` 仅满足本地 Claude 客户端配置，真实 OAuth 凭据由代理管理，不传给 Claude Code。

使用 `/model <真实模型 slug>` 切换模型，也可以使用映射后的 sonnet/opus/haiku。Gateway 模型发现可尝试 `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`；Claude Code 版本和组织策略可能过滤自定义 GPT 行，显式 `/model` 和环境变量是可靠的配置入口，实际交互仍需人工验证。

按顺序验证：

1. 简单文本与多轮问答。
2. 要求创建 `hello.txt`、读取并修改它，再读回确认内容。
3. 要求通过 Bash 执行 `pwd`、`ls`，检查工具名称和返回值。
4. 要求读取两个文件、做一次修改、执行检查，验证多轮和并行工具结果关联。
5. 故意读取不存在的文件，确认模型能收到工具错误。
6. 测试现有本地 MCP 的一个只读工具；先关闭动态 tool search。
7. 测试项目中的 Skills、Hooks 和一个 Subagent；工具权限仍由 Claude Code 负责。
8. 用 `/compact` 请求摘要，然后继续询问摘要之前的工作。
9. 退出 Claude Code，停止并重启代理，在相同测试目录使用下面的恢复命令。

## 6. `/resume` 与原项目

在相同项目目录，沿用上节的同一组环境变量，把最后的 `claude` 改成：

```sh
claude --resume
```

也可在已经配置好的 Claude Code 内执行 `/resume`。选择测试会话，要求读取刚创建的文件并继续修改。代理根据客户端重发的历史构造完整 Responses `input`，不需要旧的 Responses ID 或代理进程内会话状态。

只有上述测试通过后，再在原项目目录用相同方式恢复原会话。代理不会读取、编辑或迁移 `~/.claude/projects/`；Claude Code 正常运行自身会继续写入自己的历史。旧 provider 的不透明 thinking 签名不具备跨接口可移植性，代理省略这类隐藏推理内容，保留文本和工具历史；无法翻译的内容会返回错误。

## 7. 退出与回滚

```sh
"$CCP_BIN" siwc auth logout
# 非 default 账号：
"$CCP_BIN" siwc auth logout --profile work
```

退出会尝试官方撤销，再清除本地 Token；若远端撤销失败，终端会明确说明。随后可在 [ChatGPT 设置](https://chatgpt.com/settings/usage) 断开应用。Client ID/身份映射和 Host ID 保留供下次登录使用。新请求停止，活动流会在下一次账号检查时停止（通常一秒，锁操作等待可能更久）。

回滚时 Ctrl+C 停止代理，关闭带 SIWC 环境变量的 Claude Code 进程，重新使用原来安装的代理版本及原配置。无需删除任何历史。源码回滚可在干净工作区对本次提交执行 `git revert <交付提交>`；这不会修改 Claude Code 会话文件。撤销远端授权不能通过 Git 回滚代替。

## 8. 报错排查

| 现象 | 处理 |
| --- | --- |
| state/nonce/签名/issuer/audience 校验失败 | 停止本次授权，确认系统时间并重新登录；不要禁用校验。 |
| 没有 issued client_id | 动态注册未完成，不能用 dynamic_agent_client 交换 Token。 |
| invalid_grant（授权码） | 程序最多自动发起一次全新授权，复用本次 issued ID。仍失败则停止。 |
| refresh_token_reused/失效 | Token 清除、注册保留；对原 profile 重新登录。 |
| refresh outcome uncertain | 不重用状态不明的旋转 Token；重新授权。 |
| permission disabled | 用户决定授予套餐权限后执行 `siwc auth login --consent`。 |
| HTTP 401 | 检查所选账号、授权 scope；不会借机自动改用另一账号或 API Key。 |
| HTTP 403 | 检查用户/工作区/地区/政策限制及精确 error.code。 |
| HTTP 429 / usage_limit_exceeded | 该代理进程暂停后续推理；查看 [Usage](https://chatgpt.com/settings/usage)。额度恢复后由用户重启同账号代理；不要自动轮换账号。 |
| HTTP 503 | 对明确临时错误最多请求三次；Retry-After 超过 5 秒则立即返回，让用户稍后再试。 |
| model_not_found / 模型不可用 | 重新运行该 profile 的 `siwc models`，选择模型并实际验证；目录不保证资格。 |
| siwc_stream_error | 网络中断、缺少完成事件、无效工具关联等均失败；保留客户端历史后人工决定重试，避免重复工具副作用。 |
| unsupported tool/content | 参考兼容性表；关闭动态 tool search，使用本地 function tools。 |
| 本地 403 | 仅支持回环 CLI 请求；检查 Host、Origin、监听地址。 |
| 凭据权限错误 | 检查 siwc 目录 0700、其中普通文件 0600，不能是 symlink/hardlink；不要放宽权限。 |

本地文件：`<CCP_CONFIG_DIR>/siwc/state.json`、`state.lock`；默认是 `~/.config/claude-code-proxy/siwc/`。不要上传这些文件。本实现未启用 Keychain，不应将凭据目录共享或同步到不受信任的设备。

Claude Code 配置依据：[模型配置](https://code.claude.com/docs/en/model-config)、[环境变量](https://code.claude.com/docs/en/env-vars)、[工作流程与恢复](https://code.claude.com/docs/en/common-workflows)。
