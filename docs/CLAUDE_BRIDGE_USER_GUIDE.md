# Claude Bridge 使用手册

Claude Bridge 是独立开源、本地运行的工具，不是 OpenAI 或 Anthropic 官方产品。它让原生 Claude Code 通过官方 Sign in with ChatGPT 使用您自己的 ChatGPT 套餐。适用范围以 OpenAI 当前开源客户端接入规定、账号资格和套餐限制为准。

## 安装

支持 Apple Silicon Mac、macOS 13 或更新版本。正式分发版本应经过 Developer ID 签名和 Apple 公证。标有“本地测试构建”的版本尚未达到公开分发要求；不要关闭 Gatekeeper 或降低系统安全保护。

1. 打开 `Claude Bridge.dmg`，把 `Claude Bridge.app` 拖到 Applications（也支持用户的 Applications 文件夹）。不要直接从磁盘映像运行。
2. 打开应用，跟随首次启动向导。如果未安装 Claude Code，点击“一键安装 Claude Code”，阅读并确认第三方软件安装许可。应用从 Anthropic 官方渠道安装原生稳定版，不捆绑或修改 Claude Code。
3. 点击“安装 Claude GPT”。确认用户级命令、按需服务和 shell PATH 修改。应用会先检查冲突并备份修改的 shell 配置。不需要管理员密码。
4. 点击 **Continue with ChatGPT**，在系统浏览器中的 OpenAI 官方页面用您自己的账号登录并授权套餐。不要把验证码或凭据发给他人。
5. 确认首次套餐提示，选择从当前账号官方目录读取的 GPT 模型，点击“测试连接”。测试会消耗一次少量套餐用量，不读取项目。
6. 显示“配置完成，可以使用”后，点击“选择项目启动”，或打开一个新终端，在项目目录运行 `claude-gpt`。首次启动原生 Claude Code 时，其项目访问、工具执行等安全确认仍需您本人完成。

不需要安装 Rust、Node.js、Homebrew、Xcode Command Line Tools 或项目源码。向导中的“验证终端配置”会自动检查新 zsh 终端能否找到命令。已有终端窗口可能需要重新打开；应用提供“打开新终端”。Terminal.app、iTerm2 和 Ghostty 的 zsh 会读取用户的 PATH 配置。

## 日常使用

```text
claude-gpt
claude-gpt --continue
claude-gpt --resume
claude-gpt -p "请简短说明这个项目"
```

命令保留当前项目目录和原生 Claude Code 会话位置；应用不修改或迁移项目历史。`--resume` 的会话必须属于当前项目。原生 Claude Code 的账号、订阅和功能限制仍由 Anthropic 决定。

`claude` 与 `claude-gpt` 是两个独立入口。`claude` 使用您自己的 Anthropic 官方账号或官方 API 配置，完全不依赖 Claude Bridge。ChatGPT 授权并不等于 Claude 官方认证。Claude Bridge 只给它启动的 Claude GPT 进程设置连接信息，不向您的 shell 导出全局 API 环境变量。

可以关闭窗口或退出 Claude Bridge。按需服务和正在运行的会话独立运行；重启 Mac 后，`claude-gpt` 会自动检查并启动服务。空闲且没有 Claude GPT 会话时，服务约五分钟后退出。无需把应用加入开机登录项。

## 账号、模型与故障处理

- 账号需要重新授权时，打开应用的“ChatGPT 账号”页，再次点击 **Continue with ChatGPT**。系统按官方规则刷新正常授权；真正失效时不会切换到其他收费通道。
- 在“模型与连接”页刷新目录、选择模型并测试。更换模型前先退出正在运行的 Claude GPT 会话。默认模型不再可用时，重新选择账号当前可用模型。
- 套餐额度或访问限制请使用 **Manage usage**，打开 [ChatGPT 用量设置](https://chatgpt.com/settings/usage)。应用不会自动购买额度。
- “修复安装”会再次检查冲突、保留配置并修复命令、组件和 PATH。已有同名第三方命令、被修改的管理块、符号链接配置文件或不属于当前用户的文件不会被覆盖。
- 如果 macOS 拒绝后台服务，请在“系统设置 → 通用 → 登录项与扩展”检查后台权限后重试修复。不要关闭系统安全保护。
- “选择项目启动”会通过 Terminal.app 打开所选目录并启动 Claude GPT，无需控制其他应用的自动化权限。Claude Code 自己的安全说明与项目信任提示仍由您确认。
- “查看安全日志”打开本机日志目录。日志用于状态、协议分类与错误诊断，不应包含 OAuth Token 或项目提示词。不要向他人发送账号存储、运行时密钥或原生会话文件。

## 更新与卸载

正式发布者可配置 HTTPS 更新源；应用验证更新包 SHA-256、相同 Developer ID 团队签名和 Apple 安全评估后安装。测试版本没有公开更新源时会明确告知，不能据此判断是否最新。也可在应用中选择可信的新版 `.app` 更新。请先退出 Claude GPT 会话。应用目录必须对当前用户可写；如系统要求安装授权，使用 Finder 把已公证新版应用替换到 Applications。

在“开始使用”页选择“卸载启动器”，确认后移除本应用拥有的命令、用户级服务和 PATH 配置块。保留 OAuth、配置、备份、Claude Code 本体及历史会话。随后可以把 App 移到废纸篓。重新安装会继续使用保留的数据。要撤销 ChatGPT 授权，请在 [ChatGPT 设置](https://chatgpt.com/settings/usage) 断开应用；卸载启动器不会代表您撤销远端授权。

用户数据位于 `~/Library/Application Support/Claude Bridge`，入口位于 `~/.local/bin/claude-gpt`，按需服务定义位于 `~/Library/LaunchAgents/org.claudebridge.agent.plist`。这些位置不依赖开发者源码目录。只有主动选择“使用此 Mac 已有 SIWC 授权”的现有用户会引用之前的本机 SIWC 存储；不会复制或发布那里的凭据。
