// Run on macOS: swift -module-cache-path /tmp/lumapaint-swift-cache scripts/check-native-text-layout.swift
// Exercises AppKit overflow and mixed-size columns used by the native editor.
import AppKit
let _ = NSApplication.shared
func columns(crossExtent: CGFloat) -> [CGRect] {
    let view = NSTextView(frame: CGRect(x: 0, y: 0, width: 300, height: 500))
    view.textContainerInset = .zero
    view.string = String(repeating: "長文の文字サイズ変更。日本語 English 简体中文。", count: 100)
    view.setLayoutOrientation(.vertical)
    let container = view.textContainer!
    container.lineFragmentPadding = 0
    container.widthTracksTextView = false
    container.heightTracksTextView = false
    container.containerSize = CGSize(width: 500, height: crossExtent)
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = 28
    paragraph.maximumLineHeight = 0
    let storage = view.textStorage!
    storage.setAttributes([.font: NSFont.systemFont(ofSize: 20), .paragraphStyle: paragraph], range: NSRange(location: 0, length: storage.length))
    storage.addAttribute(.font, value: NSFont.systemFont(ofSize: 80), range: NSRange(location: 35, length: 3))
    let manager = view.layoutManager!
    manager.usesFontLeading = false
    manager.ensureLayout(for: container)
    var result: [CGRect] = []
    var glyph = 0
    while glyph < manager.numberOfGlyphs {
        var range = NSRange()
        let rect = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: &range)
        result.append(rect)
        glyph = max(glyph + 1, range.location + range.length)
    }
    return result
}
let truncated = columns(crossExtent: 300)
precondition(truncated.contains { $0.height == 0 }, "fixture must reproduce unlaid overflow glyphs")
let complete = columns(crossExtent: 100_000)
precondition(complete.count > truncated.count)
precondition(complete.allSatisfy { $0.height > 0 })
precondition(zip(complete, complete.dropFirst()).allSatisfy { $0.midY < $1.midY })
precondition(complete.contains { $0.height > 60 }, "large font must expand its column")
precondition(zip(complete, complete.dropFirst()).allSatisfy { $0.maxY <= $1.minY + 0.01 }, "column fragments must not overlap")
print("PASS: \(complete.count) mixed-size columns, complete overflow layout, increasing centers, no overlapping fragments")
// A final newline has no glyph but has an extra line fragment. Omitting it
// makes the model's visual-line count differ and discards the whole layout.
for vertical in [false, true] {
    let view = NSTextView(frame: CGRect(x: 0, y: 0, width: 500, height: 600))
    view.textContainerInset = .zero
    view.string = "日本語 English 简体中文\n\n文字サイズ変更後の文章\n"
    view.setLayoutOrientation(vertical ? .vertical : .horizontal)
    let container = view.textContainer!
    container.widthTracksTextView = false
    container.heightTracksTextView = false
    container.containerSize = CGSize(width: 500, height: 100_000)
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = 28
    paragraph.maximumLineHeight = 0
    view.textStorage!.setAttributes([.font: NSFont.systemFont(ofSize: 20), .paragraphStyle: paragraph], range: NSRange(location: 0, length: view.textStorage!.length))
    view.textStorage!.addAttribute(.font, value: NSFont.systemFont(ofSize: 80), range: NSRange(location: 23, length: 2))
    let manager = view.layoutManager!
    manager.ensureLayout(for: container)
    var count = 0
    var glyph = 0
    var previous: CGFloat = -1
    while glyph < manager.numberOfGlyphs {
        var range = NSRange()
        let rect = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: &range)
        precondition(rect.height > 0 && rect.midY > previous)
        previous = rect.midY
        count += 1
        glyph = max(glyph + 1, range.location + range.length)
    }
    let extra = manager.extraLineFragmentRect
    precondition(extra.height > 0 && extra.midY > previous)
    precondition(count == 3 && count + 1 == view.string.components(separatedBy: "\n").count)
    print("PASS: \(vertical ? "vertical" : "horizontal") terminal paragraph preserves all \(count + 1) line measurements; extra \(extra)")
}
