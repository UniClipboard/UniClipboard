import SwiftUI

/// Content for a how-to sheet — the half-screen modal a tutorial page raises to
/// walk the user through a system action it can't perform on their behalf
/// (enable keyboard, pin share extension, allow paste). Mirrors Paste's
/// "如何启用 …?" sheet: numbered steps + a stylized demo + a button that hands
/// off to the real system surface.
struct HowToContent {
    var title: LocalizedStringKey
    var steps: [LocalizedStringKey]
    var heroImage: String?
    var heroAlignment: Alignment = .bottom
    var footnote: LocalizedStringKey?
    var primaryAction: HowToAction
    var secondaryAction: HowToAction?
}

/// A button inside a how-to sheet. `kind` tells the host how to fulfill it —
/// the sheet itself stays UIKit-free; the host (`OnboardingView`) owns the
/// `openSettings` / `openShareSheet` / `dismiss` plumbing.
struct HowToAction {
    enum Kind { case openSettings, openKeyboardSettings, openShareSheet, dismiss }
    var title: LocalizedStringKey
    var kind: Kind
}

struct HowToSheet: View {
    let content: HowToContent
    var onDismiss: () -> Void
    var onAction: (HowToAction) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                GuideSheetHeader(title: content.title, onDismiss: onDismiss)

                steps.padding(.top, 18)

                if let img = content.heroImage {
                    demo(img, alignment: content.heroAlignment).padding(.top, 20)
                }
                if let note = content.footnote {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 16)
                }

                buttons.padding(.top, 20)
            }
            .padding(GuideSheetLayout.contentPadding)
        }
        .presentationDetents(GuideSheetLayout.detents)
        .presentationDragIndicator(.visible)
    }

    private var steps: some View {
        VStack(alignment: .leading, spacing: GuideSheetLayout.stepSpacing) {
            ForEach(Array(content.steps.enumerated()), id: \.offset) { idx, step in
                GuideSheetStepRow(number: idx + 1, title: step)
            }
        }
    }

    private func demo(_ imageName: String, alignment: Alignment) -> some View {
        HStack {
            Spacer()
            OnboardingHero(imageName: imageName, alignment: alignment,
                           width: 200, height: 200)
            Spacer()
        }
    }

    private var buttons: some View {
        VStack(spacing: 10) {
            GuideSheetPrimaryButton(title: content.primaryAction.title) {
                onAction(content.primaryAction)
            }

            if let sec = content.secondaryAction {
                GuideSheetSecondaryButton(title: sec.title) {
                    onAction(sec)
                }
            }
        }
    }
}

#Preview {
    Color(.systemBackground)
        .sheet(isPresented: .constant(true)) {
            HowToSheet(
                content: HowToContent(
                    title: "如何把 UniClip 置顶?",
                    steps: ["在任意 App 点「分享」", "上排滑到最右点「更多」", "右上角点「编辑」", "收藏 UniClip 并拖到顶部"],
                    heroImage: "OnboardingShare",
                    heroAlignment: .bottom,
                    footnote: nil,
                    primaryAction: HowToAction(title: "打开分享菜单试试", kind: .openShareSheet),
                    secondaryAction: nil
                ),
                onDismiss: {},
                onAction: { _ in }
            )
        }
}
