import SwiftUI

/// Guide sheet for the share extension — teaches users how to sync
/// content (text, images, files) via the system share sheet.
/// Same carousel + pulsing-dot pattern as `KeyboardSetupSheet`.
struct ShareSetupSheet: View {
    var allowsExpansion: Bool = false
    @State private var currentStep: Int = 0
    @Environment(\.dismiss) private var dismiss
    @Environment(\.colorScheme) private var colorScheme

    private struct Step: Identifiable {
        let id: Int
        let title: LocalizedStringKey
        let imageName: String
        var imageAnchor: CGFloat = 0
        var dotPosition: UnitPoint? = nil
    }

    // ── Tune per-step: imageAnchor picks the crop, dotPosition
    //    places the "tap here" indicator. ──
    private static let steps: [Step] = [
        // Step 1: Select text → tap "Share..."
        Step(id: 0, title: "选中内容，点「分享」", imageName: "ShareGuideStep1",
             imageAnchor: 0.80,
             dotPosition: UnitPoint(x: 0.42, y: 0.88)),
        // Step 2: Share sheet → tap UniClip
        Step(id: 1, title: "选择 UniClip", imageName: "ShareGuideStep2",
             imageAnchor: 0.75,
             dotPosition: UnitPoint(x: 0.45, y: 0.72)),
        // Step 3: UniClip share view → confirm & send
        Step(id: 2, title: "确认服务器，点「发送」", imageName: "ShareGuideStep3",
             imageAnchor: 0.12,
             dotPosition: UnitPoint(x: 0.88, y: 0.12)),
        // Step 4: Edit favorites → UniClip pinned
        Step(id: 3, title: "收藏到分享菜单顶部", imageName: "ShareGuideStep4",
             imageAnchor: 0.12,
             dotPosition: nil),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            GuideSheetHeader(title: "通过分享同步到服务器") { dismiss() }
            stepList.padding(.top, 18)
            stepCarousel.padding(.top, 16)
            Spacer(minLength: 0)
            GuideSheetPrimaryButton(title: "打开分享菜单试试") {
                dismiss()
            }
        }
        .padding(GuideSheetLayout.contentPadding)
        .presentationDetents(allowsExpansion ? GuideSheetLayout.expandableDetents : GuideSheetLayout.detents)
        .presentationDragIndicator(.visible)
    }

    private var stepList: some View {
        VStack(alignment: .leading, spacing: GuideSheetLayout.stepSpacing) {
            ForEach(Self.steps) { step in
                GuideSheetStepRow(
                    number: step.id + 1,
                    title: step.title,
                    isActive: step.id == currentStep
                )
                .animation(.easeInOut(duration: 0.2), value: currentStep)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation { currentStep = step.id } }
            }
        }
    }

    private var stepCarousel: some View {
        TabView(selection: $currentStep) {
            ForEach(Self.steps) { step in
                stepImage(step.imageName, anchor: step.imageAnchor, dot: step.dotPosition)
                    .tag(step.id)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .always))
        .frame(height: 260)
    }

    private func stepImage(_ name: String, anchor: CGFloat = 0, dot: UnitPoint? = nil) -> some View {
        let frameHeight: CGFloat = 230
        let imageWidthFraction: CGFloat = 0.80
        let bezel: CGFloat = 8

        return GeometryReader { geo in
            let screenW = geo.size.width * imageWidthFraction
            let deviceW = screenW + bezel * 2
            let captureAspect: CGFloat = 2622.0 / 1206.0
            let imageH = screenW * captureAspect
            let overflow = max(0, imageH - frameHeight)
            let bezelColor: Color = colorScheme == .dark
                ? Color(white: 0.26) : Color(white: 0.13)

            ZStack {
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .fill(bezelColor)
                    .frame(width: deviceW, height: frameHeight)
                    .overlay {
                        RoundedRectangle(cornerRadius: 4, style: .continuous)
                            .strokeBorder(.white.opacity(0.10), lineWidth: 0.5)
                    }

                Image(name)
                    .resizable()
                    .scaledToFill()
                    .frame(width: screenW, height: imageH)
                    .offset(y: overflow * (0.5 - anchor))
                    .frame(width: screenW, height: frameHeight)
                    .clipped()
                    .overlay {
                        if let dot {
                            GuidePulsingDot()
                                .position(x: screenW * dot.x,
                                          y: frameHeight * dot.y)
                        }
                    }
            }
            .frame(width: geo.size.width, height: frameHeight)
        }
        .frame(height: frameHeight)
        .padding(.horizontal, 4)
    }
}

#Preview {
    Color(.systemBackground)
        .sheet(isPresented: .constant(true)) {
            ShareSetupSheet()
        }
}
