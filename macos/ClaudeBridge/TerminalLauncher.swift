import Foundation

enum BridgeTerminal {
    static func quote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    static func source(project: URL?, launcher: URL) -> String {
        if let project = project {
            return "#!/bin/zsh\nset -e\ncd -- \(quote(project.path))\nexec \(quote(launcher.path))\n"
        }
        return "#!/bin/zsh\nexec /bin/zsh -l\n"
    }

    static func command(root: URL, project: URL?, launcher: URL) throws -> URL {
        let manager = FileManager.default
        let runtime = root.appendingPathComponent("runtime")
        let directory = runtime.appendingPathComponent("terminal-launches")
        for path in [root, runtime, directory] {
            try manager.createDirectory(at: path, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let attributes = try manager.attributesOfItem(atPath: path.path)
            guard attributes[.type] as? FileAttributeType == .typeDirectory,
                  (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == getuid() else {
                throw CocoaError(.fileWriteNoPermission)
            }
            try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: path.path)
        }
        // Keep the document available until Terminal reads it. Remove only our
        // own old regular files; active shells no longer read their source file.
        for file in try manager.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) {
            guard file.pathExtension == "command", UUID(uuidString: file.deletingPathExtension().lastPathComponent) != nil,
                  let attributes = try? manager.attributesOfItem(atPath: file.path),
                  attributes[.type] as? FileAttributeType == .typeRegular,
                  let date = attributes[.modificationDate] as? Date,
                  date.timeIntervalSinceNow < -86_400 else { continue }
            try? manager.removeItem(at: file)
        }
        let file = directory.appendingPathComponent(UUID().uuidString).appendingPathExtension("command")
        try Data(source(project: project, launcher: launcher).utf8).write(to: file, options: .atomic)
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: file.path)
        return file
    }
}
