// Draws the app icon (oh-key.icns source set) and the 16 pt menu icon.
// Usage: swift make-icon.swift <output folder>
import Cocoa

func drawIcon(size: Int, rounded: Bool, template: Bool) -> NSBitmapImageRep {
    let representation = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: representation)
    let bounds = NSRect(x: 0, y: 0, width: size, height: size)
    let letterColor: NSColor
    if template {
        letterColor = .black
    } else {
        let inset = CGFloat(size) * 0.06
        let background = NSBezierPath(
            roundedRect: bounds.insetBy(dx: inset, dy: inset),
            xRadius: CGFloat(size) * 0.22, yRadius: CGFloat(size) * 0.22)
        NSColor(calibratedRed: 0.13, green: 0.45, blue: 0.95, alpha: 1).setFill()
        background.fill()
        letterColor = .white
    }
    let fontSize = CGFloat(size) * (template ? 0.82 : 0.62)
    let attributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: fontSize, weight: .bold), .foregroundColor: letterColor,
    ]
    let letter = NSAttributedString(string: "V", attributes: attributes)
    let letterSize = letter.size()
    letter.draw(at: NSPoint(x: (CGFloat(size) - letterSize.width) / 2, y: (CGFloat(size) - letterSize.height) / 2))
    NSGraphicsContext.restoreGraphicsState()
    return representation
}

let outputFolder = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let iconset = outputFolder.appendingPathComponent("oh-key.iconset", isDirectory: true)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
let iconSizes: [(String, Int)] = [
    ("icon_16x16", 16), ("icon_16x16@2x", 32), ("icon_32x32", 32), ("icon_32x32@2x", 64),
    ("icon_128x128", 128), ("icon_128x128@2x", 256), ("icon_256x256", 256), ("icon_256x256@2x", 512),
    ("icon_512x512", 512), ("icon_512x512@2x", 1024),
]
for (name, size) in iconSizes {
    let data = drawIcon(size: size, rounded: true, template: false).representation(using: .png, properties: [:])!
    try data.write(to: iconset.appendingPathComponent("\(name).png"))
}
let menuIcon = drawIcon(size: 32, rounded: false, template: true)
try menuIcon.representation(using: .tiff, properties: [:])!.write(to: outputFolder.appendingPathComponent("menu-icon.tiff"))
