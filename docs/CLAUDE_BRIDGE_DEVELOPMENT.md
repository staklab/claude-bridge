# Claude Bridge 构建与维护

## 产品结构

`macos/ClaudeBridge` 是原生 SwiftUI / AppKit 菜单栏应用，最低 macOS 13，首阶段 arm64。没有 Electron、WebView、Node 或前端运行时。GUI 调用随包分发的 `claude-gpt __bridge …` 原生管理接口；这些接口是应用内部协议。

`src/bridge` 负责用户级安装、路径检查、按需服务、身份验证及原生 Claude Code 启动。`src/bin/claude-gpt.rs` 是独立入口。代理、官方 SIWC OAuth、模型目录、Token 刷新、Responses SSE 与工具转换继续复用已有模块。OAuth 仅为首次动态注册增加受控的 `Claude Bridge` 名称提示；已有注册重新授权流程保留原客户端和 Host ID。

应用使用固定用户路径：

| 内容 | 位置 |
|---|---|
| 运行组件 / 配置 / 授权 / 日志 | `~/Library/Application Support/Claude Bridge` |
| 全局入口 | `~/.local/bin/claude-gpt`，指向上述目录中的实际程序 |
| 按需 LaunchAgent | `~/Library/LaunchAgents/org.claudebridge.agent.plist` |
| 默认独立 SIWC 存储 | `…/Claude Bridge/config/siwc/state.json` |
| 原有授权复用 | 只有用户明确选择后，引用已有 SIWC 存储；不复制刷新凭据 |

用户目录和 shell 配置不能经符号链接逃逸。修改 shell 配置前检查所有权、硬链接与管理块冲突，先保存唯一备份，再原子替换。仅管理自己的 PATH 块；卸载不会把整份旧备份覆盖回用户当前文件。zsh 配置目录通过实际 `.zshenv` 检测。安装组件先验证包内清单 SHA-256，再原子替换可执行文件。

服务定义 `RunAtLoad=false`、`KeepAlive=false`，不要求 GUI 开机常驻。CLI 验证服务后复用；首次或重启后通过用户 launchd domain bootstrap / kickstart。启动锁和 daemon 锁避免并发重复启动。回环随机端口记录在 0600 文件；身份探测使用随机 challenge 的 HMAC 证明，不向未知端口发送本地密钥。只有正确身份的服务接收独立本地 Bearer 凭据。OAuth Token 永远留在代理层。身份探测不延长空闲时间；有请求或实际存活的 Claude GPT 会话时保留服务，否则约五分钟退出。

CLI 最后使用 Unix `exec` 调用未经修改的原生 Claude Code，保留 CWD、终端、PID、退出码和参数。会话租约使用 PID 与启动时间共同识别，避免重启后的 PID 复用；失效租约和私有会话设置文件会清理。仅在该进程设置本地连接、模型及必要隔离变量。额外设置文件为 0600，不修改用户或项目的 Claude 配置，也不覆盖历史。采用 `ANTHROPIC_AUTH_TOKEN`；清空该进程的 `ANTHROPIC_API_KEY`，避免原生交互模式出现额外 API Key 确认。`claude` 不经过此入口。

App 退出不会终止 launchd 管理的代理。模型切换、停止服务和卸载检查活跃会话。全局命令无需 App 运行。真正授权失效会在应用中引导重新授权，不切换接口或计费方。

## 构建

构建机需要 Xcode / Swift、Rust、Python 3（仅用于构建期许可证和测试），以及 arm64 Rust target。这些都不属于最终用户依赖。

```sh
rustup target add aarch64-apple-darwin
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
bash scripts/macos/build.sh
python3 scripts/macos/test_install.py 'dist/Claude Bridge.app'
python3 scripts/macos/audit_bundle.py 'dist/Claude Bridge.app'
```

输出实际 `dist/Claude Bridge.app`、`dist/Claude Bridge.dmg`、`dist/native/claude-gpt`、`dist/native/claude-code-proxy` 和 `dist/SHA256SUMS`。DMG 包含 Applications 拖放入口与手册。构建流程检查 arm64、生成图标、嵌入组件与许可证、校验签名，并制作磁盘映像。Rust 构建路径会映射为通用构建路径，不将开发者目录作为运行依赖。

无分发证书时使用 ad hoc 签名，只供本地开发测试，UI / DMG 明确标记尚未达到公开分发要求。不能通过移除 quarantine、关闭 Gatekeeper、降低系统安全设置代替签名公证验收。

## 签名、公证与发布

在具备 Apple Developer Program 权限的构建机，将 Developer ID Application 证书及私钥导入构建专用 Keychain，并使用 `notarytool store-credentials` 建立专用 profile；不要把证书、私钥、密码或 Apple API Key 写进仓库。

通过构建机的秘密管理注入以下值，再运行同一构建脚本：

| 变量 | 用途 |
|---|---|
| `BRIDGE_SIGN_IDENTITY` | `Developer ID Application: …` 分发身份 |
| `BRIDGE_TEAM_ID` | 对应证书的 Team ID，构建会校验匹配 |
| `BRIDGE_NOTARY_PROFILE` | 已配置的 notarytool Keychain profile |
| `BRIDGE_UPDATE_FEED` | 发布者控制的 HTTPS 更新清单 URL，可空 |

脚本顺序签名嵌套 Rust 程序、GUI 和完整 App；启用 hardened runtime。项目启动使用 Terminal 的 `.command` 文档支持，不请求 Apple Events 权限。App 压缩包提交公证、staple 并经 `spctl` 检查，再签名、公证并 staple DMG。任何必需步骤失败都会停止，不将失败包复制为交付物。只有公证通过才可对外发布。

`.github/workflows/macos-desktop.yml` 为手动触发流程。默认产出测试包；`distribution=true` 使用仓库 Secrets 创建临时 Keychain 并执行签名公证。Secrets 名称见工作流；任务最后清理证书材料。此轮不触发远端工作流、不推送或上传任何文件。

不得将任何用户 `state.json`、注册记录、Client ID、Token、Host ID、设置或本地密钥复制进包。`audit_bundle.py` 在本机构建时以已有授权中的值做不输出原值的扫描。CI 没有真实账号时只能检查禁止文件名，不能声称进行了真实凭据比对。应用本身不捆绑 Claude Code。

## 更新机制

正式更新源格式：

```json
{"version":"0.2.1","url":"https://YOUR-RELEASE-HOST/Claude-Bridge-0.2.1.dmg","sha256":"64-character-lowercase-sha256"}
```

GUI 检查 HTTPS 清单后由用户确认更新。安装前验证 DMG 摘要、App bundle ID、严格签名、与当前发行版相同的 Developer ID Team、版本递增和 `spctl` 安全评估。把 App 暂存于安装目录后替换；出现错误恢复旧 App。更新组件沿用稳定用户路径，账号与原生会话留在包外。更新前需结束 Claude GPT 会话。不具备目标目录写权限时保留原安装并引导使用 Finder 安装已公证新版。

本地测试构建未配置发布源或正式 Team ID，不会谎报“已经最新”。提供手动选择新版 `.app` 的测试更新入口；公开版本不会接受无匹配 Developer ID 的包。发布 HTTPS 清单和托管实际下载需要发布者提供受控站点及证书，本轮没有擅自建立公开渠道。

## 官方参考与支持边界

- [OpenAI 开源本地 SIWC 接入](https://developers.openai.com/siwc/token-sharing-open-source)
- [OpenAI SIWC UI / UX](https://developers.openai.com/siwc/ui-ux-guidelines)
- [OpenAI 模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [Anthropic 原生安装](https://code.claude.com/docs/en/setup)
- [Claude Code 环境变量](https://code.claude.com/docs/en/env-vars)
- [Claude Code 网关说明](https://code.claude.com/docs/en/llm-gateway)

本项目使用原生客户端提供的网关连接方式；Anthropic 文档明确不为通过网关使用非 Claude 模型提供官方支持。这是本项目自行测试维护的兼容层。OpenAI 的开源、本地客户端接入许可不应解读为对付费或远端托管商业部署的批准。
