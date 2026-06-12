import SwiftUI

/// A 截图教学页 in the walkthrough: a `PhoneFrame` "half-phone" mockup
/// occupying the top ~58% of the page (the device's framed edge — top or bottom
/// per `frameEdge` — rises out of the hero box), then a left-aligned title +
/// subtitle. The action buttons (主 CTA → how-to
/// sheet, 次按钮「稍后」) live in `OnboardingView`'s shared footer, so this view
/// renders only the upper half — keeping the footer layout identical to the
/// Symbol 卖点页.
struct OnboardingTutorialView: View {
    let tutorial: OnboardingPage.Tutorial

    var body: some View {
        GeometryReader { geo in
            VStack(spacing: 0) {
                PhoneFrame(
                    imageName: tutorial.heroImageName,
                    edge: tutorial.frameEdge,
                    verticalAnchor: tutorial.heroAnchor,
                    framedEdgeInset: tutorial.heroFramedEdgeInset
                )
                // ~58% of the page → a taller (more portrait) screen area, so
                // `scaledToFill` reveals more of the capture's vertical content
                // instead of cropping most of it away.
                .frame(height: geo.size.height * 0.58)

                Spacer(minLength: 24)

                VStack(alignment: .leading, spacing: 10) {
                    Text(tutorial.title)
                        .font(.title.weight(.bold))
                        .fixedSize(horizontal: false, vertical: true)
                    Text(tutorial.subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineSpacing(2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 32)

                Spacer()
            }
            .frame(width: geo.size.width, height: geo.size.height, alignment: .top)
        }
    }
}

#Preview("Share") { OnboardingTutorialView(tutorial: .shareExtension) }
#Preview("Keyboard") { OnboardingTutorialView(tutorial: .keyboard) }
#Preview("Paste") { OnboardingTutorialView(tutorial: .pastePermission) }
