import Foundation
import CryptoKit
let app = URL(fileURLWithPath: CommandLine.arguments[1])
let resources = app.appendingPathComponent("Contents/Resources")
var hashes: [String: String] = [:]
for name in ["claude-gpt", "claude-code-proxy"] {
    let data = try Data(contentsOf: resources.appendingPathComponent("bin/" + name))
    hashes[name] = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}
let manifest: [String: Any] = ["product": "org.claudebridge.desktop", "version": CommandLine.arguments[2], "binaries": hashes]
try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys]).write(to: resources.appendingPathComponent("manifest.json"))
let release = ["team_id": CommandLine.arguments[3], "feed_url": CommandLine.arguments[4]]
try JSONSerialization.data(withJSONObject: release, options: [.prettyPrinted, .sortedKeys]).write(to: resources.appendingPathComponent("release.json"))
