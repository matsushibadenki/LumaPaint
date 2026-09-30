// Native font matrices must advance following characters without resizing the text frame.
import AppKit
let _ = NSApplication.shared
for vertical in [false, true] {
    let view = NSTextView(frame: NSRect(x: 0, y: 0, width: 500, height: 500))
    view.string = "AAAAAA"
    view.textContainerInset = .zero
    view.textContainer!.lineFragmentPadding = 0
    view.textContainer!.heightTracksTextView = false
    view.textContainer!.containerSize = NSSize(width: 500, height: 100_000)
    view.setLayoutOrientation(vertical ? .vertical : .horizontal)
    let font = NSFont.systemFont(ofSize: 24)
    let storage = view.textStorage!
    storage.setAttributes([.font: font], range: NSRange(location: 0, length: storage.length))
    let manager = view.layoutManager!
    manager.ensureLayout(for: view.textContainer!)
    let before = manager.location(forGlyphAt: 4).x
    var matrix: [CGFloat] = [48, 0, 0, 36, 0, 0]
    let transformed = matrix.withUnsafeMutableBufferPointer { NSFont(name: font.fontName, matrix: $0.baseAddress!)! }
    storage.addAttribute(.font, value: transformed, range: NSRange(location: 2, length: 2))
    manager.ensureLayout(for: view.textContainer!)
    precondition(manager.location(forGlyphAt: 4).x > before + 10, "following text must move after changing selected glyph width")
    precondition((storage.attribute(.font, at: 0, effectiveRange: nil) as! NSFont) == font)
    precondition((storage.attribute(.font, at: 4, effectiveRange: nil) as! NSFont) == font)
    precondition(view.frame.size == NSSize(width: 500, height: 500))
}
print("PASS: horizontal/vertical selected font transforms advance following text; unselected fonts and frame preserved")
