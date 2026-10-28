import AppKit
import CryptoKit
import Foundation

enum BridgeUpdater {
    struct Feed: Decodable { let version: String; let url: URL; let sha256: String }
    private static func configuration(_ bundle: Bundle) -> [String: String] {
        guard let url = bundle.url(forResource: "release", withExtension: "json"), let data = try? Data(contentsOf: url), let value = try? JSONSerialization.jsonObject(with: data) as? [String: String] else { return [:] }
        return value
    }
    @MainActor
    static func check(bundle: Bundle) async throws -> String {
        let config = configuration(bundle)
        guard let source = config["feed_url"], let url = URL(string: source), url.scheme == "https", config["team_id"]?.isEmpty == false else {
            return "本地测试构建尚未配置已签名的公开更新源，不能判断是否为最新版。可以选择已验证的新版 Claude Bridge.app 进行更新。"
        }
        var request = URLRequest(url: url); request.timeoutInterval = 30
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200, response.url?.scheme == "https", data.count < 32_768 else { throw BridgeError.message("更新源返回异常，请稍后重试。") }
        let feed = try JSONDecoder().decode(Feed.self, from: data)
        guard feed.url.scheme == "https", feed.sha256.count == 64, feed.sha256.allSatisfy({ $0.isHexDigit }) else { throw BridgeError.message("更新清单格式异常。") }
        let current = bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        guard feed.version.compare(current, options: .numeric) == .orderedDescending else { return "当前已是更新源提供的最新版本（\(current)）。" }
        let alert = NSAlert(); alert.messageText = "Claude Bridge \(feed.version) 可用"
        alert.informativeText = "将下载更新并验证 SHA-256、Developer ID 与 Apple 安全评估。请先退出正在运行的 Claude GPT 会话。账号和会话数据会保留。"
        alert.addButton(withTitle: "下载并更新"); alert.addButton(withTitle: "稍后")
        guard alert.runModal() == .alertFirstButtonReturn else { return "已取消更新。" }
        let (file, downloadResponse) = try await URLSession.shared.download(from: feed.url)
        guard (downloadResponse as? HTTPURLResponse)?.statusCode == 200, downloadResponse.url?.scheme == "https" else { throw BridgeError.message("下载更新失败。") }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("claude-bridge-update-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let dmg = directory.appendingPathComponent("update.dmg")
        try FileManager.default.moveItem(at: file, to: dmg)
        let bytes = try Data(contentsOf: dmg, options: .mappedIfSafe)
        guard SHA256.hash(data: bytes).map({ String(format: "%02x", $0) }).joined() == feed.sha256.lowercased() else { throw BridgeError.message("更新包完整性校验失败，未安装。") }
        let mount = directory.appendingPathComponent("mounted")
        _ = try await command("/usr/bin/hdiutil", ["attach", dmg.path, "-nobrowse", "-readonly", "-mountpoint", mount.path])
        do {
            let app = mount.appendingPathComponent("Claude Bridge.app")
            try await install(source: app, current: bundle.bundleURL)
            _ = try? await command("/usr/bin/hdiutil", ["detach", mount.path])
        } catch { _ = try? await command("/usr/bin/hdiutil", ["detach", mount.path]); throw error }
        return "更新已安装。请重新打开 Claude Bridge。"
    }

    static func command(_ executable: String, _ arguments: [String]) async throws -> Data {
        try await Task.detached(priority: .userInitiated) {
            let process = Process(); process.executableURL = URL(fileURLWithPath: executable); process.arguments = arguments
            let output = Pipe(); process.standardOutput = output; process.standardError = FileHandle.nullDevice; process.standardInput = FileHandle.nullDevice
            try process.run(); let data = output.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
            guard process.terminationStatus == 0 else { throw BridgeError.message("更新验证或安装失败。请检查应用签名、目录写入权限，并先退出 Claude GPT 会话。现有安装已保留。") }
            return data
        }.value
    }

    static func install(source: URL, current: URL) async throws {
        guard source.standardizedFileURL != current.standardizedFileURL, let candidate = Bundle(url: source), candidate.bundleIdentifier == "org.claudebridge.desktop", let existing = Bundle(url: current) else { throw BridgeError.message("请选择完整的 Claude Bridge.app 更新包。") }
        let newVersion = candidate.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        let oldVersion = existing.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
        guard newVersion.compare(oldVersion, options: .numeric) == .orderedDescending else { throw BridgeError.message("请选择版本高于当前安装的更新包。") }
        _ = try await command("/usr/bin/codesign", ["--verify", "--deep", "--strict", source.path])
        let team = configuration(existing)["team_id"] ?? ""
        if !team.isEmpty {
            let requirement = "anchor apple generic and certificate leaf[subject.OU] = \"" + team + "\" and identifier \"org.claudebridge.desktop\""
            _ = try await command("/usr/bin/codesign", ["--verify", "-R", requirement, source.path])
            _ = try await command("/usr/sbin/spctl", ["--assess", "--type", "execute", source.path])
        } else {
            guard candidate.object(forInfoDictionaryKey: "BridgePublicDistribution") as? Bool != true else { throw BridgeError.message("当前是测试构建，请从正式分发渠道安装已公证版本。") }
        }
        let backend = source.appendingPathComponent("Contents/Resources/bin/claude-gpt")
        _ = try await command(backend.path, ["__bridge", "stop"])
        let parent = current.deletingLastPathComponent()
        let staged = parent.appendingPathComponent(".ClaudeBridge-update-" + UUID().uuidString + ".app")
        let backup = parent.appendingPathComponent(".ClaudeBridge-previous-" + UUID().uuidString + ".app")
        let fm = FileManager.default
        try fm.copyItem(at: source, to: staged)
        defer { try? fm.removeItem(at: staged) }
        do {
            try fm.moveItem(at: current, to: backup)
            do {
                try fm.moveItem(at: staged, to: current)
                let installed = current.appendingPathComponent("Contents/Resources/bin/claude-gpt")
                _ = try await command(installed.path, ["__bridge", "install", current.path, "--consent"])
            } catch {
                try? fm.removeItem(at: current); try fm.moveItem(at: backup, to: current); throw error
            }
            try? fm.removeItem(at: backup)
        } catch { throw error }
    }
}
