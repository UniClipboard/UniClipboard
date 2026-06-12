import SwiftUI

/// A full-bleed "hero" screenshot for the onboarding教学页 — a real device
/// screenshot (light/dark variants live in `Assets.xcassets`) framed into a
/// square-ish window. `scaledToFill` + `alignment` pick which slice of the full
/// screen shows, so the source asset stays an uncropped 1206×2622 capture and
/// re-framing is a code change, not a re-crop:
/// - `.bottom` → keyboard / share sheet (salient UI sits at the screen bottom)
/// - `.top`    → settings pages (salient rows sit near the top)
///
/// Replaces the old SwiftUI-drawn `DeviceFrame`/`OnboardingDemo`. Trade-off:
/// screenshots no longer auto-localize, so demos ship one language; light/dark
/// is still automatic via the asset catalog's appearance slots.
struct OnboardingHero: View {
    let imageName: String
    var alignment: Alignment = .bottom
    let width: CGFloat
    let height: CGFloat
    /// Page hero bleeds from the top edge → round only the bottom corners.
    /// How-to sheet card floats → round all four.
    var bottomOnlyRounding = false

    private let radius: CGFloat = 28

    var body: some View {
        Image(imageName)
            .resizable()
            .scaledToFill()
            .frame(width: width, height: height, alignment: alignment)
            .clipped()
            .clipShape(clipShape)
    }

    private var clipShape: AnyShape {
        if bottomOnlyRounding {
            AnyShape(UnevenRoundedRectangle(
                bottomLeadingRadius: radius,
                bottomTrailingRadius: radius,
                style: .continuous))
        } else {
            AnyShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        }
    }
}

#Preview("Keyboard") {
    OnboardingHero(imageName: "OnboardingKeyboard", alignment: .bottom,
                   width: 390, height: 360, bottomOnlyRounding: true)
}
#Preview("Paste card") {
    OnboardingHero(imageName: "OnboardingPaste", alignment: .top,
                   width: 200, height: 200)
}
