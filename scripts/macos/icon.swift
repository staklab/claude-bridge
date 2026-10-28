import AppKit
import Foundation
let directory = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = size * scale
        let image = NSImage(size: NSSize(width: pixels, height: pixels))
        image.lockFocus()
        let p = CGFloat(pixels)
        NSColor(calibratedRed: 0.07, green: 0.14, blue: 0.24, alpha: 1).setFill()
        NSBezierPath(roundedRect: NSRect(x: p * 0.04, y: p * 0.04, width: p * 0.92, height: p * 0.92), xRadius: p * 0.21, yRadius: p * 0.21).fill()
        let path = NSBezierPath(); path.lineWidth = p * 0.065; path.lineCapStyle = .round
        path.move(to: NSPoint(x: p * 0.22, y: p * 0.32)); path.line(to: NSPoint(x: p * 0.22, y: p * 0.57))
        path.curve(to: NSPoint(x: p * 0.78, y: p * 0.57), controlPoint1: NSPoint(x: p * 0.22, y: p * 0.94), controlPoint2: NSPoint(x: p * 0.78, y: p * 0.94))
        path.line(to: NSPoint(x: p * 0.78, y: p * 0.32))
        NSColor(calibratedRed: 0.45, green: 0.76, blue: 1, alpha: 1).setStroke(); path.stroke()
        NSColor.white.setFill()
        for x in [0.22, 0.5, 0.78] { NSBezierPath(ovalIn: NSRect(x: p * (x - 0.055), y: p * 0.265, width: p * 0.11, height: p * 0.11)).fill() }
        image.unlockFocus()
        let bitmap = NSBitmapImageRep(data: image.tiffRepresentation!)!
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("icon_\(size)x\(size)\(scale == 2 ? "@2x" : "").png"))
    }
}
