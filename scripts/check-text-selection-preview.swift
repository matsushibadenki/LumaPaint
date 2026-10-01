// macOS: verifies the selection painting hook with focus in a numeric field.
import AppKit
let _ = NSApplication.shared
final class PreviewLayoutManager: NSLayoutManager {
    var paintedRects: [NSRect] = []
    override func drawGlyphs(forGlyphRange range: NSRange, at origin: NSPoint) {
        let selection = firstTextView!.selectedRange()
        addTemporaryAttribute(.foregroundColor, value: NSColor.white, forCharacterRange: selection)
        super.drawGlyphs(forGlyphRange: range, at: origin)
        removeTemporaryAttribute(.foregroundColor, forCharacterRange: selection)
    }
    override func fillBackgroundRectArray(_ rectArray: UnsafePointer<NSRect>, count rectCount: Int,
                                         forCharacterRange charRange: NSRange, color: NSColor) {
        paintedRects.append(contentsOf: UnsafeBufferPointer(start: rectArray, count: rectCount))
        NSColor.black.setFill()
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
view.selectedTextAttributes = [.foregroundColor: NSColor.white,
                              .backgroundColor: NSColor.black]
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
    var whitePixels = 0
    var blackPixels = 0
    for y in 0..<bitmap.pixelsHigh {
        for x in 0..<bitmap.pixelsWide {
            let color = bitmap.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
            alpha = max(alpha, color.alphaComponent)
            if color.alphaComponent > 0.9 {
                if color.redComponent > 0.9 && color.greenComponent > 0.9 && color.blueComponent > 0.9 { whitePixels += 1 }
                if color.redComponent < 0.1 && color.greenComponent < 0.1 && color.blueComponent < 0.1 { blackPixels += 1 }
            }
        }
    }
    precondition(alpha == 1 && whitePixels > 0 && blackPixels > 0, "selection must show white glyphs over black, including inactive focus")
    return bitmap
}
let _ = snapshot()
let before = manager.paintedRects.map(\.height).max()!
view.textStorage!.addAttribute(.font, value: NSFont.systemFont(ofSize: 60), range: selection)
view.needsDisplay = true
let _ = snapshot()
precondition(manager.paintedRects.map(\.height).max()! > before, "selection must follow changed font metrics")
precondition(view.selectedRange() == selection, "editing the size must preserve the selected range")
view.setLayoutOrientation(.vertical)
view.frame = NSRect(x: 0, y: 40, width: 100, height: 500)
view.setBoundsSize(NSSize(width: 100, height: 500))
view.textContainer!.widthTracksTextView = false
view.textContainer!.heightTracksTextView = false
view.textContainer!.containerSize = NSSize(width: 100_000, height: 100_000)
let _ = snapshot()
precondition(view.selectedRange() == selection)
// Point text uses the unbounded inline axis; only explicit newlines make new lines.
for vertical in [false, true] {
    view.setLayoutOrientation(vertical ? .vertical : .horizontal)
    view.string = String(repeating: "あいうえお", count: 30)
    view.textContainer!.widthTracksTextView = false
    view.textContainer!.heightTracksTextView = false
    view.textContainer!.containerSize = NSSize(width: 100_000, height: 100_000)
    manager.ensureLayout(for: view.textContainer!)
    var lines = 0
    manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { _, _, _, _, _ in lines += 1 }
    precondition(lines == 1, "point text must not wrap at the former frame edge")
}
window.orderOut(nil)
window.close()
print("PASS: white selected glyphs on black after size changes; range and updated geometry preserved")
