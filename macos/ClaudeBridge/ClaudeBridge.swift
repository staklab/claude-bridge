import AppKit
import SwiftUI
import UniformTypeIdentifiers

@main
struct ClaudeBridgeApp: App {
    @StateObject private var bridge = BridgeModel()
    var body: some Scene {
        WindowGroup("Claude Bridge", id: "setup") {
            BridgeWindow().environmentObject(bridge).frame(minWidth: 820, minHeight: 650)
        }.windowStyle(.hiddenTitleBar).defaultSize(width: 880, height: 700)
        MenuBarExtra("Claude Bridge", systemImage: "point.3.connected.trianglepath.dotted") {
            MenuContent().environmentObject(bridge)
        }
    }
}

struct MenuContent: View {
    @EnvironmentObject var bridge: BridgeModel
    @Environment(\.openWindow) var openWindow
    var body: some View {
        Text(bridge.ready ? "Using ChatGPT plan · \(bridge.selectedModel)" : "Claude Bridge · 待配置")
        Text(bridge.serviceRunning ? "本地连接正常" : "本地服务按需启动")
        Divider()
        Button("打开 Claude Bridge") { openWindow(id: "setup"); NSApp.activate(ignoringOtherApps: true) }
        Button("选择项目启动…") { bridge.chooseProject() }.disabled(!bridge.ready || bridge.busy)
        Button("Manage usage") { bridge.openUsage() }
        Divider()
        Button("退出应用") { NSApp.terminate(nil) }.keyboardShortcut("q")
    }
}

@MainActor
final class BridgeModel: ObservableObject {
    @Published var state: [String: Any] = [:]
    @Published var catalog: [[String: Any]] = []
    @Published var selectedModel = ""
    @Published var busy = false
    @Published var progress = ""
    @Published var message = ""
    @Published var failure = false
    @Published var showWelcome = false
    @Published var selectedPage = 0
    @Published var terminal = "com.apple.Terminal"
    @Published var updateMessage = ""
    private var activeProcess: Process?
    private var loaded = false
    var installed: Bool { state["installed"] as? Bool == true && state["launcher_installed"] as? Bool == true }
    var nativeInstalled: Bool { state["claude_path"] is String }
    var account: [String: Any] { state["account"] as? [String: Any] ?? [:] }
    var signedIn: Bool { account["signed_in"] as? Bool == true }
    var permitted: Bool { account["plan_permission"] as? Bool == true }
    var serviceRunning: Bool { (state["service"] as? [String: Any])?["verified"] as? Bool == true }
    var ready: Bool { installed && nativeInstalled && permitted && !selectedModel.isEmpty && state["tested_model"] as? String == selectedModel }
    var shellReady: Bool { (state["shell"] as? [String: Any])?["configured"] as? Bool == true }
    var usageLimited: Bool { failure && (message.contains("HTTP 429") || message.localizedCaseInsensitiveContains("usage_limit")) }
    var root: URL { FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Claude Bridge") }
    var launcher: URL { root.appendingPathComponent("bin/claude-gpt") }
    var executable: URL { Bundle.main.resourceURL!.appendingPathComponent("bin/claude-gpt") }
    var version: String { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.2.1" }
    var localBuild: Bool { Bundle.main.object(forInfoDictionaryKey: "BridgePublicDistribution") as? Bool != true }

    func load() { guard !loaded else { return }; loaded = true; refresh() }
    private func process(_ arguments: [String], binary: URL? = nil) async throws -> Data {
        let process = Process()
        process.executableURL = binary ?? executable
        process.arguments = ["__bridge"] + arguments
        var environment = ProcessInfo.processInfo.environment
        environment["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
        process.environment = environment
        process.standardInput = FileHandle.nullDevice
        let output = Pipe(), errors = Pipe()
        process.standardOutput = output; process.standardError = errors
        activeProcess = process
        return try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    try process.run()
                    let group = DispatchGroup(), capture = ProcessCapture()
                    group.enter()
                    DispatchQueue.global().async { capture.stdout = output.fileHandleForReading.readDataToEndOfFile(); group.leave() }
                    group.enter()
                    DispatchQueue.global().async { capture.stderr = errors.fileHandleForReading.readDataToEndOfFile(); group.leave() }
                    process.waitUntilExit(); group.wait()
                    if process.terminationStatus == 0 { continuation.resume(returning: capture.stdout) }
                    else { continuation.resume(throwing: BridgeError.message(Self.safeError(capture.stderr))) }
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
    nonisolated private static func safeError(_ data: Data) -> String {
        let text = String(decoding: data, as: UTF8.self)
        if ["Bearer ", "access_token", "refresh_token", "id_token", "https://auth.openai.com/"].contains(where: text.contains) {
            return "授权未完成。请重试 Continue with ChatGPT；已有账号记录已保留。"
        }
        return String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(900)).isEmpty ? "操作未完成，请重试。" : String(text.prefix(900))
    }
    private func json(_ args: [String]) async throws -> [String: Any] { try JSONSerialization.jsonObject(with: await process(args)) as? [String: Any] ?? [:] }
    private func reload() async throws {
        state = try await json(["status"])
        selectedModel = state["model"] as? String ?? ""
    }
    private func fetchModels() async throws {
        let data = try await process(["models"])
        catalog = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] ?? []
        if catalog.isEmpty { throw BridgeError.message("此账号暂未返回可用 GPT 模型，请检查套餐权限后重试。") }
        if !selectedModel.isEmpty && !catalog.contains(where: { $0["id"] as? String == selectedModel }) {
            throw BridgeError.message("当前默认模型已不在此账号的目录中，请选择其他模型并测试。")
        }
    }
    private func action(_ title: String, _ work: @escaping () async throws -> Void) {
        guard !busy else { return }
        busy = true; progress = title; message = ""; failure = false
        Task {
            do { try await work() }
            catch { failure = true; message = error.localizedDescription }
            busy = false; progress = ""; activeProcess = nil
        }
    }
    func refresh() {
        action("正在检测本机安装和授权状态…") {
            try await self.reload()
            self.selectedPage = !self.installed || !self.nativeInstalled ? 0 : !self.permitted ? 1 : self.ready ? 3 : 2
            if self.permitted {
                try await self.fetchModels()
                if self.state["plan_notice_seen"] as? Bool != true { self.showWelcome = true }
            }
        }
    }
    func install() {
        action("正在检查安装位置与 shell 配置…") {
            let preview = try await self.json(["install-plan"])
            let files = (preview["shell_files"] as? [String] ?? []).joined(separator: "\n")
            let alert = NSAlert(); alert.messageText = "安装 Claude GPT 命令？"
            alert.informativeText = "将在用户目录安装 claude-gpt 和按需服务，并为新终端添加 PATH。修改前会备份配置，不需要管理员密码。\n\n\(files)\n\n原生 claude 命令与会话不会被修改。"
            alert.addButton(withTitle: "同意并安装"); alert.addButton(withTitle: "取消")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            self.progress = "正在安装原生组件和用户级启动器…"
            _ = try await self.json(["install", Bundle.main.bundleURL.path, "--consent"])
            try await self.reload()
            self.message = "Claude GPT 已安装。新打开的终端会自动识别命令。"
        }
    }
    func installClaude() {
        let alert = NSAlert(); alert.messageText = "安装 Anthropic 官方 Claude Code？"
        alert.informativeText = "Claude Code 是独立的第三方软件。应用将从 claude.ai 下载并运行官方原生稳定版安装器。无需 Node.js、Homebrew 或开发工具。Claude 原生模式仍需要您自己的 Anthropic 认证。"
        alert.addButton(withTitle: "同意并安装"); alert.addButton(withTitle: "取消")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        action("正在通过 Anthropic 官方渠道下载并安装 Claude Code…") {
            _ = try await self.json(["install-claude", "--consent"])
            try await self.reload(); self.message = "Claude Code 已安装，可以继续登录 ChatGPT。"
        }
    }
    func login() {
        action("请在系统浏览器完成 ChatGPT 登录与套餐授权…") {
            _ = try await self.process(["auth-login"])
            try await self.reload()
            guard self.permitted else { throw BridgeError.message("账号尚未授予套餐推理权限。请重新授权并确认使用 ChatGPT 套餐；账号也可能暂不符合官方 SIWC 接入条件。") }
            try await self.fetchModels()
            self.selectedPage = 2
            self.showWelcome = self.state["plan_notice_seen"] as? Bool != true
            self.message = "授权成功。请选择此账号的模型并测试连接。"
        }
    }
    func adoptExisting() {
        let alert = NSAlert(); alert.messageText = "使用此 Mac 已有 SIWC 授权？"
        alert.informativeText = "仅适用于本机已使用 claude-code-proxy 完成官方 SIWC 授权的用户。应用将引用同一授权存储，不复制凭据，不重新注册客户端。新用户请使用 Continue with ChatGPT。"
        alert.addButton(withTitle: "使用已有授权"); alert.addButton(withTitle: "取消")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        action("正在连接此 Mac 的已有授权…") {
            _ = try await self.json(["use-existing-auth", "--consent"])
            try await self.reload(); try await self.fetchModels()
            self.showWelcome = self.permitted && self.state["plan_notice_seen"] as? Bool != true
            self.selectedPage = 2
        }
    }
    func acknowledge() { showWelcome = false; action("正在保存…") { _ = try await self.json(["notice-seen"]); try await self.reload() } }
    func cancelLogin() { activeProcess?.terminate() }
    func chooseModel(_ model: String) {
        guard !model.isEmpty && model != selectedModel else { return }
        action("正在保存默认 GPT 模型…") { _ = try await self.json(["set-model", model]); try await self.reload() }
    }
    func test() {
        action("正在向 OpenAI 重新检查额度并完成一次轻量推理测试…") {
            _ = try await self.json(["test"]); try await self.reload()
            self.message = "连接测试通过，可以使用 Claude GPT。"
        }
    }
    func models() { action("正在获取此账号的官方模型目录…") { try await self.fetchModels() } }
    func verifyShell() {
        action("正在验证新终端能否识别 claude-gpt…") {
            let value = try await self.json(["verify-shell"])
            guard value["verified"] as? Bool == true else { throw BridgeError.message("终端配置尚未生效，可能存在自定义启动脚本覆盖 PATH。请点击修复安装；如仍失败，请打开应用日志寻求支持。") }
            self.message = "已验证：新的 zsh 终端能直接运行 claude-gpt。现有窗口请重新打开。"
        }
    }
    func uninstall() {
        let alert = NSAlert(); alert.messageText = "卸载 Claude GPT 启动器？"
        alert.informativeText = "将移除本应用的全局命令、按需服务和 PATH 配置块。保留您的 ChatGPT 授权、Claude Code 和全部项目会话。应用本身可随后移到废纸篓。"
        alert.addButton(withTitle: "卸载启动器"); alert.addButton(withTitle: "取消")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        action("正在卸载启动器并保留用户数据…") { _ = try await self.json(["uninstall", "--consent"]); try await self.reload(); self.message = "启动器已卸载，账号与会话数据已保留。" }
    }
    func stopService() { action("正在停止空闲服务…") { _ = try await self.json(["stop"]); try await self.reload() } }
    func openUsage() { NSWorkspace.shared.open(URL(string: "https://chatgpt.com/settings/usage")!) }
    func openLogs() { try? FileManager.default.createDirectory(at: root.appendingPathComponent("logs"), withIntermediateDirectories: true); NSWorkspace.shared.open(root.appendingPathComponent("logs")) }
    func chooseProject() {
        let panel = NSOpenPanel(); panel.title = "选择 Claude GPT 项目"; panel.canChooseFiles = false; panel.canChooseDirectories = true
        guard panel.runModal() == .OK, let project = panel.url else { return }
        openTerminal(project: project)
    }
    func openTerminal(project: URL? = nil) {
        // Project launch uses Terminal's native .command document support, with no
        // Apple Events permission or interpolation through a second script language.
        let bundleID = project == nil ? terminal : "com.apple.Terminal"
        guard let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else { failure = true; message = "未找到所选终端，请选择 Terminal.app 或安装该终端后重试。"; return }
        message = ""; failure = false
        let options = NSWorkspace.OpenConfiguration()
        if bundleID != "com.apple.Terminal" {
            options.createsNewApplicationInstance = true
            NSWorkspace.shared.openApplication(at: app, configuration: options) { _, error in
                if let error = error { Task { @MainActor in self.failure = true; self.message = "无法打开终端（系统错误 \((error as NSError).code)），请检查终端安装后重试。" } }
            }
            return
        }
        do {
            let script = try BridgeTerminal.command(root: root, project: project, launcher: launcher)
            NSWorkspace.shared.open([script], withApplicationAt: app, configuration: options) { _, error in
                if let error = error {
                    try? FileManager.default.removeItem(at: script)
                    Task { @MainActor in self.failure = true; self.message = "无法打开终端（系统错误 \((error as NSError).code)），请检查 Terminal.app 后重试。" }
                }
            }
        } catch { failure = true; message = "无法创建用户级终端启动文件，请点击修复安装后重试。" }
    }
    func checkUpdate() {
        action("正在检查应用更新…") {
            self.updateMessage = try await BridgeUpdater.check(bundle: .main)
            self.message = self.updateMessage
        }
    }
    func installUpdate() {
        let panel = NSOpenPanel(); panel.title = "选择新版 Claude Bridge.app"; panel.allowedContentTypes = [.applicationBundle]; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        action("正在验证并安装应用更新…") { try await BridgeUpdater.install(source: url, current: Bundle.main.bundleURL); self.message = "更新已安装。请重新打开 Claude Bridge；现有会话与授权已保留。" }
    }
}

final class ProcessCapture: @unchecked Sendable { var stdout = Data(); var stderr = Data() }
enum BridgeError: LocalizedError { case message(String); var errorDescription: String? { if case let .message(s) = self { return s }; return nil } }

struct BridgeWindow: View {
    @EnvironmentObject var bridge: BridgeModel
    @Environment(\.colorScheme) var colorScheme
    private let pages = [("1", "安装", "shippingbox"), ("2", "ChatGPT 账号", "person.crop.circle"), ("3", "模型与连接", "point.3.connected.trianglepath.dotted"), ("4", "开始使用", "terminal")]
    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 26) {
                HStack(spacing: 10) { Image(systemName: "point.3.connected.trianglepath.dotted").font(.system(size: 25, weight: .medium)).foregroundStyle(.blue); Text("Claude\nBridge").font(.system(size: 20, weight: .semibold, design: .rounded)) }.padding(.top, 24)
                Text("YOUR TOOLS.\nYOUR CHATGPT PLAN.").font(.system(size: 10, weight: .semibold, design: .monospaced)).tracking(1.1).foregroundStyle(.secondary)
                VStack(spacing: 7) {
                    ForEach(pages.indices, id: \.self) { i in
                        Button { bridge.selectedPage = i } label: {
                            HStack(spacing: 10) { Image(systemName: pages[i].2).frame(width: 20); Text(pages[i].1); Spacer() }
                                .font(.system(size: 13, weight: bridge.selectedPage == i ? .semibold : .regular)).padding(11).foregroundStyle(bridge.selectedPage == i ? Color.accentColor : .primary).background(bridge.selectedPage == i ? Color.accentColor.opacity(0.10) : .clear).clipShape(RoundedRectangle(cornerRadius: 8))
                        }.buttonStyle(.plain)
                    }
                }
                Spacer()
                Text("Apple Silicon · \(bridge.version)").font(.caption).foregroundStyle(.secondary)
                if bridge.localBuild { Text("本地测试构建\n尚未完成公开分发签名与公证").font(.caption2).foregroundStyle(.orange) }
                Text("独立开源应用\n非 OpenAI / Anthropic 官方产品").font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }.padding(22).frame(width: 205).background(Color(nsColor: .controlBackgroundColor))
            Divider()
            VStack(alignment: .leading, spacing: 20) {
                HStack { Text("\(bridge.selectedPage + 1) / 4").font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary); Spacer(); Button { bridge.refresh() } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.borderless).help("刷新状态").disabled(bridge.busy) }.padding(.top, 20)
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        if bridge.selectedPage == 0 { installation }
                        if bridge.selectedPage == 1 { account }
                        if bridge.selectedPage == 2 { model }
                        if bridge.selectedPage == 3 { launch }
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(.bottom, 12)
                }
                if bridge.busy { HStack(spacing: 10) { ProgressView().controlSize(.small); Text(bridge.progress).font(.callout); if bridge.progress.contains("浏览器") { Button("取消") { bridge.cancelLogin() }.buttonStyle(.borderless) } }.padding(12).frame(maxWidth: .infinity, alignment: .leading).background(Color.accentColor.opacity(0.06)).clipShape(RoundedRectangle(cornerRadius: 10)) }
                if !bridge.message.isEmpty { HStack(alignment: .top, spacing: 8) { Image(systemName: bridge.failure ? "exclamationmark.circle" : "checkmark.circle"); Text(bridge.message).textSelection(.enabled); if bridge.failure { Button("Manage usage") { bridge.openUsage() }.buttonStyle(.link) } }.font(.callout).foregroundStyle(bridge.failure ? .orange : .secondary).padding(12).background(Color(nsColor: .controlBackgroundColor)).clipShape(RoundedRectangle(cornerRadius: 10)) }
                Divider()
                HStack { Text("退出应用后，命令和正在运行的会话仍可使用。").font(.caption).foregroundStyle(.secondary); Spacer(); if bridge.selectedPage < 3 { Button("继续") { bridge.selectedPage += 1 }.buttonStyle(.borderedProminent).disabled(bridge.busy || (bridge.selectedPage == 0 && (!bridge.installed || !bridge.nativeInstalled)) || (bridge.selectedPage == 1 && !bridge.permitted) || (bridge.selectedPage == 2 && !bridge.ready)) } }
            }.padding(.horizontal, 30).padding(.bottom, 22)
        }.onAppear { bridge.load() }.sheet(isPresented: $bridge.showWelcome) { welcome }.tint(.blue)
    }
    private func title(_ eyebrow: String, _ title: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 10) { Text(eyebrow).font(.system(size: 11, weight: .medium, design: .monospaced)).tracking(1.4).foregroundStyle(.blue); Text(title).font(.system(size: 29, weight: .semibold)); Text(detail).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).lineSpacing(3) }
    }
    private func status(_ label: String, _ detail: String, _ good: Bool) -> some View {
        HStack(spacing: 12) { Image(systemName: good ? "checkmark.circle.fill" : "circle.dotted").foregroundStyle(good ? .green : .secondary); Text(label).fontWeight(.medium); Spacer(); Text(detail).font(.callout).foregroundStyle(.secondary).lineLimit(1) }.padding(14).background(Color(nsColor: .controlBackgroundColor)).clipShape(RoundedRectangle(cornerRadius: 10))
    }
    private var installation: some View {
        VStack(alignment: .leading, spacing: 16) {
            title("WELCOME", "让熟悉的工具，连接 GPT。", "只需完成这一次设置。之后在任意项目目录输入 claude-gpt，就能使用 ChatGPT 套餐驱动的原生 Claude Code。")
            status("Claude Code", bridge.nativeInstalled ? "已检测到官方程序" : "需要安装", bridge.nativeInstalled)
            if !bridge.nativeInstalled { Button("一键安装 Claude Code…") { bridge.installClaude() }.buttonStyle(.bordered).disabled(bridge.busy) }
            status("Claude GPT", bridge.installed ? "已安装全局命令" : "等待安装", bridge.installed)
            Button(bridge.installed ? "修复安装…" : "安装 Claude GPT…") { bridge.install() }.buttonStyle(.borderedProminent).disabled(bridge.busy)
            Text("用户级安装，无需管理员密码。仅为 claude-gpt 设置连接信息；原生 claude 继续使用您自己的 Anthropic 认证。").font(.caption).foregroundStyle(.secondary).lineSpacing(3)
            if bridge.installed { HStack { Button("验证终端配置") { bridge.verifyShell() }; Button("打开新终端") { bridge.openTerminal() } }.disabled(bridge.busy) }
        }
    }
    private var account: some View {
        VStack(alignment: .leading, spacing: 18) {
            title("SIGN IN", "使用你自己的 ChatGPT 套餐。", "登录会在系统浏览器中的 OpenAI 官方页面完成。应用在本机保存授权，每个用户和设备独立授权。")
            status("ChatGPT 账号", bridge.signedIn ? (bridge.account["email"] as? String ?? "已登录") : "未登录", bridge.signedIn)
            status("套餐推理权限", bridge.permitted ? "已授权" : "待授权 / 检查资格", bridge.permitted)
            Button { bridge.login() } label: { HStack(spacing: 10) { ChatGPTMark(white: true).frame(width: 20, height: 20); Text("Continue with ChatGPT").font(.system(size: 14, weight: .semibold)) }.padding(.horizontal, 22).padding(.vertical, 12).foregroundStyle(.white).background(.black).clipShape(RoundedRectangle(cornerRadius: 9)) }.buttonStyle(.plain).disabled(!bridge.installed || bridge.busy)
            HStack { Button("Manage usage") { bridge.openUsage() }.buttonStyle(.link); Text("在 ChatGPT 中管理应用权限与用量").font(.caption).foregroundStyle(.secondary) }
            Text("ChatGPT 授权仅用于 Claude GPT，不代表完成了 Claude 官方账号认证。服务受 OpenAI 当前 SIWC 资格与套餐限制约束。").font(.caption).foregroundStyle(.secondary).lineSpacing(3)
            if bridge.state["legacy_authorization_available"] as? Bool == true && !bridge.signedIn { Button("使用此 Mac 已有 SIWC 授权…") { bridge.adoptExisting() }.buttonStyle(.link).disabled(bridge.busy || !bridge.installed) }
        }
    }
    private var model: some View {
        VStack(alignment: .leading, spacing: 18) {
            title("CONNECT", "选择模型，确认连接。", "模型目录来自当前授权账号。测试将使用您的 ChatGPT 套餐完成一次简短推理，不读取任何项目内容。")
            HStack { Text("默认 GPT 模型").fontWeight(.medium); Spacer(); Button("刷新目录") { bridge.models() }.buttonStyle(.link).disabled(bridge.busy || !bridge.permitted) }
            Picker("默认模型", selection: Binding(get: { bridge.selectedModel }, set: { bridge.chooseModel($0) })) {
                Text("请选择可用模型").tag("")
                ForEach(bridge.catalog.compactMap { $0["id"] as? String }, id: \.self) { id in Text(id).tag(id) }
            }.labelsHidden().frame(maxWidth: .infinity).disabled(bridge.busy || !bridge.permitted)
            HStack { Label("Using ChatGPT plan", systemImage: "person.crop.circle.badge.checkmark").font(.caption); Spacer(); Button("Manage usage") { bridge.openUsage() }.buttonStyle(.link) }.foregroundStyle(.secondary)
            status("本地代理", bridge.serviceRunning ? "身份已验证 · 运行正常" : "按需启动", bridge.serviceRunning)
            status("轻量推理测试", bridge.ready ? "已通过" : "等待测试", bridge.ready)
            if bridge.usageLimited {
                HStack { Button("Manage usage") { bridge.openUsage() }.buttonStyle(.borderedProminent); Button("重新检查额度") { bridge.test() }.buttonStyle(.bordered).disabled(bridge.busy || bridge.selectedModel.isEmpty || !bridge.permitted) }
            } else {
                Button("测试连接") { bridge.test() }.buttonStyle(.borderedProminent).disabled(bridge.busy || bridge.selectedModel.isEmpty || !bridge.permitted)
            }
            Text("代理会在需要时启动，空闲后退出。正常情况下会自动刷新授权；如果官方要求重新授权，请回到账号页面登录。").font(.caption).foregroundStyle(.secondary).lineSpacing(3)
        }
    }
    private var launch: some View {
        VStack(alignment: .leading, spacing: 16) {
            title(bridge.ready ? "READY" : "ALMOST THERE", bridge.ready ? "配置完成，可以使用。" : "完成前面的设置后即可使用。", "选择项目开始，或在新的终端中使用全局命令。无需每次打开 Claude Bridge。")
            Text("claude-gpt").font(.system(size: 23, weight: .medium, design: .monospaced)).padding(20).frame(maxWidth: .infinity, alignment: .leading).background(Color(nsColor: .controlBackgroundColor)).clipShape(RoundedRectangle(cornerRadius: 12))
            Button("选择项目启动（Terminal）…") { bridge.chooseProject() }.buttonStyle(.borderedProminent).disabled(!bridge.ready || bridge.busy)
            HStack { Picker("终端", selection: $bridge.terminal) { Text("Terminal.app").tag("com.apple.Terminal"); Text("iTerm2").tag("com.googlecode.iterm2"); Text("Ghostty").tag("com.mitchellh.ghostty") }; Button("打开新终端") { bridge.openTerminal() } }
            Text("支持 --resume、--continue、-p 等原生参数。claude 命令独立运行，仍由 Anthropic 官方认证或您的官方 API 配置提供服务。").font(.caption).foregroundStyle(.secondary).lineSpacing(3)
            Divider()
            Text("维护").font(.headline)
            HStack { Button("更新应用") { bridge.checkUpdate() }; Button("从新版 App 更新…") { bridge.installUpdate() }; Button("修复安装…") { bridge.install() } }.disabled(bridge.busy)
            HStack { Button("查看安全日志") { bridge.openLogs() }; Button("停止空闲代理") { bridge.stopService() }.disabled(bridge.busy || !bridge.serviceRunning); Button("卸载启动器…", role: .destructive) { bridge.uninstall() }.disabled(bridge.busy || !bridge.installed) }
        }
    }
    private var welcome: some View {
        VStack(spacing: 22) {
            ChatGPTMark(white: colorScheme == .dark).frame(width: 42, height: 42)
            Text("You’re using your ChatGPT plan").font(.title2.bold()).multilineTextAlignment(.center)
            Text("Claude Bridge 中符合条件的 AI 请求会使用您的 ChatGPT 套餐。您可以随时在 ChatGPT 设置中查看用量、调整限制或撤销应用权限。").foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("Manage usage") { bridge.openUsage() }.buttonStyle(.link)
            Button("Got it") { bridge.acknowledge() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
        }.padding(36).frame(width: 440).interactiveDismissDisabled()
    }
}

struct ChatGPTMark: View {
    let white: Bool
    var body: some View {
        if let url = Bundle.main.url(forResource: white ? "chatgpt-logo-white" : "chatgpt-logo-black", withExtension: "svg"), let image = NSImage(contentsOf: url) {
            Image(nsImage: image).resizable().scaledToFit().accessibilityHidden(true)
        }
    }
}
