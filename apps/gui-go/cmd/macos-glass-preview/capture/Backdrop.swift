// Places a synthetic, high-contrast backdrop window directly below a given window, so a region
// screenshot shows what that window's material samples without exposing other windows' content.
// Usage: backdrop <windowNumber> <x> <y> <width> <height> <seconds> [activate]
// With "activate" this helper becomes the active app, so the target window shows its inactive state.
import AppKit

let a = CommandLine.arguments
let target = Int(a[1])!
let frameTopLeft = NSRect(x: Double(a[2])!, y: Double(a[3])!, width: Double(a[4])!, height: Double(a[5])!)
let seconds = Double(a[6])!

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

final class Stripes: NSView {
    override func draw(_ dirtyRect: NSRect) {
        let colors: [NSColor] = [.systemRed, .systemOrange, .systemYellow, .systemGreen, .systemBlue, .systemPurple]
        let band = bounds.width / CGFloat(colors.count * 2)
        for i in 0..<(colors.count * 2) {
            colors[i % colors.count].setFill()
            NSRect(x: CGFloat(i) * band, y: 0, width: band, height: bounds.height).fill()
        }
        let text = "BACKDROP" as NSString
        text.draw(at: NSPoint(x: 40, y: bounds.height / 2),
                  withAttributes: [.font: NSFont.boldSystemFont(ofSize: 120), .foregroundColor: NSColor.white])
    }
}

// Convert the top-left based CGWindow bounds to AppKit's bottom-left screen coordinates.
let screenHeight = NSScreen.screens[0].frame.height
let frame = NSRect(x: frameTopLeft.minX, y: screenHeight - frameTopLeft.maxY,
                   width: frameTopLeft.width, height: frameTopLeft.height)
let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
window.contentView = Stripes(frame: NSRect(origin: .zero, size: frame.size))
window.isOpaque = true
window.hasShadow = false
window.order(.below, relativeTo: target)
if a.count > 7 && a[7] == "activate" { app.activate(ignoringOtherApps: true) }
print("backdrop window \(window.windowNumber) below \(target)")
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { exit(0) }
app.run()
