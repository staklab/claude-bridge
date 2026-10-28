import Foundation

@main
struct TerminalTests {
    static func main() throws {
        let manager = FileManager.default
        let temporary = manager.temporaryDirectory.appendingPathComponent("bridge-terminal-\(UUID().uuidString)")
        try manager.createDirectory(at: temporary, withIntermediateDirectories: true)
        defer { try? manager.removeItem(at: temporary) }
        let root = temporary.appendingPathComponent("application data")
        let project = temporary.appendingPathComponent("project ' $(false) `false` ; \"\nline")
        try manager.createDirectory(at: project, withIntermediateDirectories: true)
        let launcher = temporary.appendingPathComponent("launcher ' $() ;")
        try Data("#!/bin/zsh\n/bin/pwd -P\n".utf8).write(to: launcher)
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: launcher.path)
        let file = try BridgeTerminal.command(root: root, project: project, launcher: launcher)
        let attributes = try manager.attributesOfItem(atPath: file.path)
        precondition((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700)
        let process = Process(), pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = [file.path]; process.standardOutput = pipe
        try process.run()
        let output = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        precondition(process.terminationStatus == 0)
        let actual = String(decoding: output, as: UTF8.self)
        guard let canonical = realpath(project.path, nil) else { throw CocoaError(.fileReadUnknown) }
        defer { free(canonical) }
        precondition(actual == String(cString: canonical) + "\n")
        precondition(BridgeTerminal.source(project: nil, launcher: launcher) == "#!/bin/zsh\nexec /bin/zsh -l\n")
        let otherRoot = temporary.appendingPathComponent("symlink case")
        try manager.createDirectory(at: otherRoot, withIntermediateDirectories: true)
        try manager.createSymbolicLink(at: otherRoot.appendingPathComponent("runtime"), withDestinationURL: root.appendingPathComponent("runtime"))
        do {
            _ = try BridgeTerminal.command(root: otherRoot, project: project, launcher: launcher)
            preconditionFailure("symlink runtime must be rejected")
        } catch CocoaError.fileWriteNoPermission { }
        print("{\"passed\":4,\"checks\":[\"private executable document\",\"quoted project and launcher preserve cwd\",\"new login shell\",\"symlink runtime rejected\"]}")
    }
}
