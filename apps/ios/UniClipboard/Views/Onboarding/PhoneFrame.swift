import SwiftUI

/// A "half-phone" iPhone mockup for the onboarding 教学页 hero. Renders only the
/// TOP or BOTTOM portion of a device; the opposite edge is sheared flat so the
/// phone reads as rising out of (or descending into) the hero area.
///
/// - `.bottom` — bottom corners rounded, top sheared, no island. For UI that
///   docks at the screen bottom (keyboard) → pair with a high `verticalAnchor`.
/// - `.top` — top corners rounded **plus a Dynamic Island**, bottom sheared.
///   For a screen whose head matters (share sheet) → the island reads as the
///   phone's top; `verticalAnchor` then picks which slice shows through.
///
/// Layout contract: an equal `inset` on the three framed sides; the sheared
/// side is flush to the hero box edge. `verticalAnchor` (0 = top … 1 = bottom)
/// scrolls the full-screen capture within the screen window, so the salient row
/// (e.g. the share sheet's app row) can be dialed into view without re-cropping.
struct PhoneFrame: View {
    enum Edge { case top, bottom }

    let imageName: String
    var edge: Edge = .bottom
    /// Which slice of the full-screen capture shows: 0 = top, 0.5 = middle,
    /// 1 = bottom. The capture is width-fit, so this only scrolls vertically.
    var verticalAnchor: CGFloat = 0
    /// Equal gap on the three framed sides inside the hero box; the sheared side
    /// is flush. A larger inset narrows the screen, which shrinks the capture
    /// and reveals MORE of it at once.
    var inset: CGFloat = 52
    /// Gap on the rounded/solid edge specifically — the one opposite the shear
    /// (bottom for `.bottom`, top for `.top`). Defaults to `inset`; set 0 to
    /// seat that edge flush against the hero box (e.g. the keyboard page rests
    /// its rounded bottom on the hero bottom with no gap). Left/right keep
    /// `inset` regardless.
    var framedEdgeInset: CGFloat? = nil
    /// Metal-bezel thickness around the three framed edges.
    var bezel: CGFloat = 11
    /// Screen corner radius; the shell's outer radius is this + `bezel`.
    var screenRadius: CGFloat = 46
    /// Source capture aspect (h / w). All onboarding captures are full-screen
    /// iPhone shots (1206×2622 or the same-ratio 2412×5244).
    var captureAspect: CGFloat = 2622.0 / 1206.0

    @Environment(\.colorScheme) private var scheme

    private var isTop: Bool { edge == .top }

    var body: some View {
        GeometryReader { geo in
            // Device frame: full width minus a gap each side; full height minus
            // one gap on the rounded/solid side (the sheared side is flush).
            let edgeInset = framedEdgeInset ?? inset
            let fw = max(0, geo.size.width - inset * 2)
            let fh = max(0, geo.size.height - edgeInset)
            // Screen window inset by the bezel on its framed sides.
            let sw = max(0, fw - bezel * 2)
            let sh = max(0, fh - bezel)
            // Capture is width-fit → taller than the window; scroll it by anchor.
            let captureH = sw * captureAspect
            let overflow = max(0, captureH - sh)

            // Screen hugs the FRAMED edge: bottom-frame → top, top-frame → bottom.
            ZStack(alignment: isTop ? .bottom : .top) {
                shellShape
                    .fill(bezelColor)
                    .frame(width: fw, height: fh)
                    .overlay {
                        shellShape
                            .strokeBorder(.white.opacity(0.12), lineWidth: 1)
                            .frame(width: fw, height: fh)
                    }

                Color.clear
                    .frame(width: sw, height: sh)
                    .overlay {
                        Image(imageName)
                            .resizable()
                            .scaledToFill()
                            .frame(width: sw, height: captureH)
                            // anchor 0 → push down (top visible); 1 → up (bottom).
                            .offset(y: overflow * (0.5 - verticalAnchor))
                    }
                    .clipShape(screenShape)
                    .overlay(alignment: .top) {
                        if isTop { dynamicIsland(screenWidth: sw) }
                    }
            }
            // Pin the device to the sheared edge (flush) and center horizontally
            // → equal gap on the three framed sides, no gap on the shear.
            .frame(width: geo.size.width, height: geo.size.height,
                   alignment: isTop ? .bottom : .top)
        }
    }

    /// Round only the framed edge's corners; the sheared edge is a straight cut.
    private var shellShape: UnevenRoundedRectangle {
        let r = screenRadius + bezel
        return isTop
            ? UnevenRoundedRectangle(topLeadingRadius: r, topTrailingRadius: r, style: .continuous)
            : UnevenRoundedRectangle(bottomLeadingRadius: r, bottomTrailingRadius: r, style: .continuous)
    }

    private var screenShape: UnevenRoundedRectangle {
        isTop
            ? UnevenRoundedRectangle(topLeadingRadius: screenRadius, topTrailingRadius: screenRadius, style: .continuous)
            : UnevenRoundedRectangle(bottomLeadingRadius: screenRadius, bottomTrailingRadius: screenRadius, style: .continuous)
    }

    /// Black pill mimicking the Dynamic Island, sized off the screen width so it
    /// tracks the (scaled-down) capture. Drawn over the status-bar area.
    private func dynamicIsland(screenWidth: CGFloat) -> some View {
        Capsule(style: .continuous)
            .fill(.black)
            .frame(width: screenWidth * 0.32, height: screenWidth * 0.32 * 0.30)
            .padding(.top, screenWidth * 0.03)
    }

    /// Real-device bezels are dark metal; lighten in dark mode so the shell
    /// separates from the (dark) hero background instead of melting into it.
    private var bezelColor: Color {
        scheme == .dark ? Color(white: 0.26) : Color(white: 0.13)
    }
}

#Preview("Share · top edge (app row)") {
    PhoneFrame(imageName: "OnboardingShare", edge: .top, verticalAnchor: 0.5)
        .frame(height: 420)
        .padding()
}

#Preview("Keyboard · bottom edge") {
    PhoneFrame(imageName: "OnboardingKeyboard", edge: .bottom, verticalAnchor: 1)
        .frame(height: 420)
        .padding()
}
