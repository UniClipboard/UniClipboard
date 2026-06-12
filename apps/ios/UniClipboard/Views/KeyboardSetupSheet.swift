import SwiftUI
import UIKit

/// Keyboard extension state, read from App Group flags that the keyboard
/// writes on every `viewDidAppear`.
enum KeyboardExtensionStatus {
    case notEnabled
    case enabledWithoutFullAccess
    case fullyEnabled

    private static let keyboardBundleID = "app.uniclipboard.UniClipboard.Keyboard"

    static var current: KeyboardExtensionStatus {
        guard isKeyboardInSystemList else { return .notEnabled }
        let group = UserDefaults(suiteName: SettingsStore.appGroupID)
        if group?.bool(forKey: AppSettings.PersistenceKey.keyboardExtensionFullAccess) == true {
            return .fullyEnabled
        }
        return .enabledWithoutFullAccess
    }

    /// Reads the system's enabled-keyboards list — updated the moment the
    /// user toggles the keyboard in Settings, no keyboard open required.
    private static var isKeyboardInSystemList: Bool {
        guard let keyboards = UserDefaults.standard.object(forKey: "AppleKeyboards") as? [String] else {
            return false
        }
        return keyboards.contains(keyboardBundleID)
    }
}

/// Half-sheet that guides the user through enabling the UniClip keyboard.
/// Adapts its content based on the keyboard's current state:
/// - Not enabled → 4-step carousel with swipeable screenshots.
/// - Enabled without Full Access → single-step Full Access guide.
/// - Fully enabled → same as not-enabled (re-setup reference).
struct KeyboardSetupSheet: View {
    var allowsExpansion: Bool = false
    @State private var status: KeyboardExtensionStatus
    @State private var currentStep: Int = 0
    @Environment(\.dismiss) private var dismiss
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.scenePhase) private var scenePhase

    init(status: KeyboardExtensionStatus, allowsExpansion: Bool = false) {
        _status = State(initialValue: status)
        self.allowsExpansion = allowsExpansion
    }

    private struct Step: Identifiable {
        let id: Int
        let title: LocalizedStringKey
        let imageName: String
        /// Which vertical slice of the full-screen capture to show.
        /// 0 = top edge, 0.5 = middle, 1 = bottom edge.
        var imageAnchor: CGFloat = 0
        /// Position of the pulsing dot indicator as a fraction of the
        /// visible image frame (0,0 = top-left, 1,1 = bottom-right).
        /// nil = no dot.
        var dotPosition: UnitPoint? = nil
    }

    // ── Tune these per-step: imageAnchor picks the crop, dotPosition
    //    places the "tap here" indicator. ──
    private static let fullSetupSteps: [Step] = [
        // Step 1: Settings > UniClip — point to "Keyboards" row
        Step(id: 0, title: "打开「设置」，选择「键盘」", imageName: "KeyboardGuideStep1",
             imageAnchor: 0.2,
             dotPosition: UnitPoint(x: 0.5, y: 0.55)),
        // Step 2: Keyboards — point to UniClip toggle
        Step(id: 1, title: "允许 UniClip", imageName: "KeyboardGuideStep2",
             imageAnchor: 0.08,
             dotPosition: UnitPoint(x: 0.88, y: 0.35)),
        // Step 3: UniClip on — point to "Allow Full Access" toggle
        Step(id: 2, title: "允许完全访问", imageName: "KeyboardGuideStep3",
             imageAnchor: 0.08,
             dotPosition: UnitPoint(x: 0.88, y: 0.51)),
        // Step 4: Confirmation dialog — point to "Allow" button
        Step(id: 3, title: "确认允许", imageName: "KeyboardGuideStep4",
             imageAnchor: 0.55,
             dotPosition: UnitPoint(x: 0.80, y: 0.80)),
    ]

    var body: some View {
        Group {
            switch status {
            case .enabledWithoutFullAccess:
                fullAccessContent
            case .notEnabled, .fullyEnabled:
                fullSetupContent
            }
        }
        .presentationDetents(allowsExpansion ? GuideSheetLayout.expandableDetents : GuideSheetLayout.detents)
        .presentationDragIndicator(.visible)
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { status = .current }
        }
    }

    // MARK: - Full Setup (4-step carousel)

    private var fullSetupContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            GuideSheetHeader(title: "如何启用键盘扩展？") { dismiss() }
            stepList.padding(.top, 18)
            stepCarousel.padding(.top, 16)
            Spacer(minLength: 0)
            GuideSheetPrimaryButton(title: "打开设置", action: openKeyboardSettings)
        }
        .padding(GuideSheetLayout.contentPadding)
    }

    private var stepList: some View {
        VStack(alignment: .leading, spacing: GuideSheetLayout.stepSpacing) {
            ForEach(Self.fullSetupSteps) { step in
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
            ForEach(Self.fullSetupSteps) { step in
                stepImage(step.imageName, anchor: step.imageAnchor, dot: step.dotPosition)
                    .tag(step.id)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .always))
        .frame(height: 260)
    }

    /// Renders a full-screen capture framed by left/right phone bezels,
    /// with an optional pulsing dot overlay.
    /// - `anchor`: 0 = top, 0.5 = center, 1 = bottom of the capture.
    /// - `dot`: fractional position inside the visible screen; nil = no dot.
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

    // MARK: - Full Access Only

    private var fullAccessContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            GuideSheetHeader(title: "允许完全访问") { dismiss() }

            Text("UniClip 需要完整的访问权限才能安全地同步并支持所有类型的内容。它永不向任何人（包括开发人员）传送任何敏感信息。")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 14)

            stepImage("KeyboardGuideFullAccess", anchor: 0, dot: UnitPoint(x: 0.88, y: 0.51))
                .padding(.top, 20)
            Spacer(minLength: 0)
            GuideSheetPrimaryButton(title: "打开设置", action: openKeyboardSettings)
        }
        .padding(GuideSheetLayout.contentPadding)
    }

    // MARK: - Actions

    private func openKeyboardSettings() {
        let keyboard = URL(string: "App-prefs:General&path=Keyboard")
        let fallback = URL(string: UIApplication.openSettingsURLString)
        guard let url = keyboard ?? fallback else { return }
        UIApplication.shared.open(url)
    }
}

#Preview("Not Enabled") {
    Color(.systemBackground)
        .sheet(isPresented: .constant(true)) {
            KeyboardSetupSheet(status: .notEnabled)
        }
}

#Preview("No Full Access") {
    Color(.systemBackground)
        .sheet(isPresented: .constant(true)) {
            KeyboardSetupSheet(status: .enabledWithoutFullAccess)
        }
}
