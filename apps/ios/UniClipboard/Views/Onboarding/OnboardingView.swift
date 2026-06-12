import SwiftUI
import UIKit

/// The onboarding walkthrough container — a horizontally-paged carousel that
/// renders one of three sequences depending on `Mode`.
///
/// - **`.firstRun`** — mounted by `ContentView` on a truly fresh install (no
///   server configured). Pages = the 3 卖点页 only. Last page「开始使用」calls
///   `onFinish` (= `vm.completeOnboarding()`), then ContentView routes to the
///   SetupFlow as a separate step.
/// - **`.enhancements`** — auto-presented once by `ContentView` right after the
///   first-run pairing. Pages = the 3 截图教学页 (keyboard → share → paste). A
///   close button shows; each page's CTA raises a how-to sheet and the last
///   page's secondary「完成」calls `onFinish` (= dismiss the sheet).
/// - **`.review`** — presented from Settings via `fullScreenCover`. Pages = 卖点
///   + 教学 (the full tour). Close button shows; last page's
///   secondary「完成」calls `onFinish` (= dismiss).
///
/// Env hooks (screenshots only): `UC_ONBOARDING_PAGE=n` lands on page `n`;
/// `UC_ONBOARDING_HOWTO=1` auto-raises the current page's how-to sheet.
struct OnboardingView: View {
    enum Mode {
        /// Fresh-install 卖点 walkthrough, purely informational.
        case firstRun
        /// Post-pairing 教学 carousel ("解锁更多"), shown once.
        case enhancements
        /// Settings re-view: full 卖点 + 教学 tour.
        case review
    }

    let mode: Mode
    var onFinish: () -> Void

    @State private var selection: Int
    @State private var activeTutorial: OnboardingPage.Tutorial?
    @State private var showHowTo = false

    private let pages: [OnboardingPage]

    init(mode: Mode, onFinish: @escaping () -> Void) {
        self.mode = mode
        self.onFinish = onFinish
        let pages = Self.pages(for: mode)
        self.pages = pages
        let initial = ProcessInfo.processInfo.environment["UC_ONBOARDING_PAGE"].flatMap(Int.init) ?? 0
        _selection = State(initialValue: max(0, min(pages.count - 1, initial)))
    }

    private static func pages(for mode: Mode) -> [OnboardingPage] {
        switch mode {
        case .firstRun:     return OnboardingPage.features
        case .enhancements: return OnboardingPage.tutorials
        case .review:       return OnboardingPage.all
        }
    }

    private var isLastPage: Bool { selection >= pages.count - 1 }
    private var showsCloseBar: Bool { mode != .firstRun }

    var body: some View {
        VStack(spacing: 0) {
            TabView(selection: $selection) {
                ForEach(Array(pages.enumerated()), id: \.element.id) { idx, page in
                    pageView(page).tag(idx)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))

            footer
        }
        .background(backgroundGradient)
        .overlay(alignment: .topTrailing) {
            if showsCloseBar { closeButton }
        }
        .task {
            let env = ProcessInfo.processInfo.environment
            if env["UC_ONBOARDING_HOWTO"] == "1", case .tutorial(let t) = pages[selection] {
                presentHowTo(t)
            }
        }
        .sheet(isPresented: $showHowTo) {
            if let t = activeTutorial {
                switch t {
                case .keyboard:
                    KeyboardSetupSheet(status: .current)
                case .shareExtension:
                    ShareSetupSheet()
                case .pastePermission:
                    PasteSetupSheet()
                }
            }
        }
    }

    private var closeButton: some View {
        Button(action: onFinish) {
            Image(systemName: "xmark.circle.fill")
                .font(.title2)
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(.secondary)
        }
        .accessibilityLabel("关闭")
        .padding(.horizontal, 20)
        .padding(.top, 12)
    }

    @ViewBuilder
    private func pageView(_ page: OnboardingPage) -> some View {
        switch page {
        case .feature(let f):  OnboardingFeatureView(feature: f)
        case .tutorial(let t): OnboardingTutorialView(tutorial: t)
        }
    }

    private var footer: some View {
        VStack(spacing: 14) {
            PageIndicator(count: pages.count, current: selection)
            footerButtons
        }
        .padding(.horizontal, 32)
        .padding(.top, 8)
        .padding(.bottom, 24)
    }

    @ViewBuilder
    private var footerButtons: some View {
        switch pages[selection] {
        case .feature:
            ctaButton(isLastPage ? "开始使用" : "继续", action: advance)
        case .tutorial(let t):
            VStack(spacing: 10) {
                ctaButton(t.primaryCTA) { presentHowTo(t) }
                secondaryButton(
                    isLastPage ? "完成" : "稍后",
                    action: isLastPage ? onFinish : advance
                )
            }
        }
    }

    private func ctaButton(_ title: LocalizedStringKey, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.body.weight(.semibold))
                .padding(.horizontal, 36)
                .padding(.vertical, 12)
                .foregroundStyle(Color(.systemBackground))
        }
        .buttonStyle(.borderedProminent)
    }

    private func secondaryButton(_ title: LocalizedStringKey, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline.weight(.medium))
                .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.secondary)
    }

    private var backgroundGradient: some View {
        LinearGradient(
            colors: [Color.accentColor.opacity(0.10), .clear],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
    }

    // MARK: - Actions

    private func advance() {
        if isLastPage {
            onFinish()
        } else {
            withAnimation { selection += 1 }
        }
    }

    private func presentHowTo(_ t: OnboardingPage.Tutorial) {
        if t == .pastePermission {
            _ = UIPasteboard.general.string
        }
        activeTutorial = t
        showHowTo = true
    }

}

private struct PageIndicator: View {
    let count: Int
    let current: Int

    var body: some View {
        HStack(spacing: 8) {
            ForEach(0..<count, id: \.self) { i in
                Circle()
                    .fill(i == current ? Color.accentColor : Color.secondary.opacity(0.3))
                    .frame(width: 8, height: 8)
            }
        }
        .animation(.easeInOut(duration: 0.2), value: current)
    }
}

#Preview("First-run") {
    OnboardingView(mode: .firstRun) {}
}

#Preview("Enhancements") {
    OnboardingView(mode: .enhancements) {}
}

#Preview("Review") {
    OnboardingView(mode: .review) {}
}
