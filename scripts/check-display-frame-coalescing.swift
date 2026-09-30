// macOS 14+: exercises the view-attached display link and latest-state scheduling.
// Run on the desktop host: swift -module-cache-path /tmp/lumapaint-swift-cache scripts/check-display-frame-coalescing.swift
import AppKit
final class FrameProbe: NSView {
    var latest = 0
    var pending = false
    var rendered: [Int] = []
    var link: CADisplayLink?
    func request(_ value: Int) {
        latest = value
        pending = true
        if link == nil {
            link = displayLink(target: self, selector: #selector(tick(_:)))
            link!.add(to: .main, forMode: .common)
        }
        link!.isPaused = false
    }
    @objc func tick(_ link: CADisplayLink) {
        if pending {
            pending = false
            rendered.append(latest)
        }
        if !pending { link.isPaused = true }
    }
}
let app = NSApplication.shared
let window = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 160, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
window.title = "LumaPaint Frame QA"
window.isReleasedWhenClosed = false
let view = FrameProbe(frame: window.contentView!.bounds)
window.contentView!.addSubview(view)
window.orderFront(nil)
for value in 1...1000 { view.request(value) }
func waitFor(_ count: Int) {
    let deadline = Date().addingTimeInterval(3)
    while view.rendered.count < count && Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    precondition(view.rendered.count == count, "display link did not render exactly one latest-state frame")
}
waitFor(1)
precondition(view.rendered == [1000] && view.link!.isPaused)
for value in 1001...2000 { view.request(value) }
waitFor(2)
precondition(view.rendered == [1000, 2000] && view.link!.isPaused)
view.link!.invalidate()
window.close()
print("PASS: 2000 preview requests -> 2 latest-state display frames; link pauses when idle")
