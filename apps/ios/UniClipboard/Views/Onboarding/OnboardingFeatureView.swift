import SwiftUI

/// A single 卖点页 in the onboarding walkthrough. Pure text layout — large bold
/// title centered vertically with a secondary subtitle below. Apple "What's New"
/// style, no symbols or illustrations.
struct OnboardingFeatureView: View {
    let feature: OnboardingPage.Feature

    var body: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(spacing: 20) {
                Text(feature.title)
                    .font(.system(size: 34, weight: .bold))
                    .multilineTextAlignment(.center)

                Text(feature.subtitle)
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .lineSpacing(4)
            }
            .padding(.horizontal, 8)

            Spacer()
            Spacer()
        }
        .padding(.horizontal, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

#Preview {
    OnboardingFeatureView(feature: .crossPlatform)
}
