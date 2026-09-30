// macOS: verifies the selection painting hook with focus in a numeric field.
import AppKit
let _ = NSApplication.shared
final class PreviewLayoutManager: NSLayoutManager {
    var paintedRects: [NSRect] = []
    override func fillBackgroundRectArray(_ rectArray: UnsafePointer<NSRect>, count rectCount: Int,
                                         forCharacterRange charRange: NSRange, color: NSColor) {
        paintedRects.append(contentsOf: UnsafeBufferPointer(start: rectArray, count: rectCount))
        NSColor(srgbRed: 0.15, green: 0.5, blue: 1, alpha: 0.25).setFill()
        for rect in UnsafeBufferPointer(start: rectArray, count: rectCount) { rect.fill(using: .sourceOver) }
    }
}
let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 240),
                      styleMask: [.titled], backing: .buffered, defer: false)
window.title = "LumaPaint Selection Preview QA"
let view = NSTextView(frame: NSRect(x: 0, y: 40, width: 460, height: 180))
view.drawsBackground = false
view.string = "English 日本語 简体中文 Preview"
let manager = PreviewLayoutManager()
view.textContainer!.replaceLayoutManager(manager)
view.textStorage!.setAttributes([.font: NSFont.systemFont(ofSize: 20), .foregroundColor: NSColor.clear],
                                range: NSRange(location: 0, length: view.textStorage!.length))
view.selectedTextAttributes = [.foregroundColor: NSColor.clear,
                              .backgroundColor: NSColor(srgbRed: 0.15, green: 0.5, blue: 1, alpha: 0.25)]
let field = NSTextField(frame: NSRect(x: 10, y: 5, width: 100, height: 25))
window.contentView!.addSubview(view)
window.contentView!.addSubview(field)
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(view)
let selection = NSRange(location: 8, length: 3)
view.setSelectedRange(selection)
window.makeFirstResponder(field)
func snapshot() -> NSBitmapImageRep {
    manager.paintedRects.removeAll()
    manager.ensureLayout(for: view.textContainer!)
    let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds)!
    view.cacheDisplay(in: view.bounds, to: bitmap)
    precondition(!manager.paintedRects.isEmpty, "inactive selection must use the painting hook")
    var alpha: CGFloat = 0
    for y in 0..<bitmap.pixelsHigh {
        for x in 0..<bitmap.pixelsWide {
            alpha = max(alpha, bitmap.colorAt(x: x, y: y)!.alphaComponent)
        }
    }
    precondition(alpha > 0 && alpha < 0.6, "inactive selection must not cover GPU glyphs opaquely: \(alpha)")
    return bitmap
}
let _ = snapshot()
let before = manager.paintedRects.map(\.height).max()!
view.textStorage!.addAttribute(.font, value: NSFont.systemFont(ofSize: 60), range: selection)
view.needsDisplay = true
let _ = snapshot()
precondition(manager.paintedRects.map(\.height).max()! > before, "selection must follow changed font metrics")
precondition(view.selectedRange() == selection, "editing the size must preserve the selected range")
window.orderOut(nil)
window.close()
print("PASS: inactive selection stays translucent after size changes; range and updated geometry preserved")
