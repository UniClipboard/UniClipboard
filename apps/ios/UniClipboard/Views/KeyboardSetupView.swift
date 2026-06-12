import SwiftUI
import UIKit

/// Keyboard extension guide page — shows a visual preview of the UniClip
/// keyboard and a CTA that opens a setup sheet adapted to the current
/// keyboard state. Accessible from Settings → "键盘与自动同步" and planned
/// for reuse in the onboarding flow.
struct KeyboardSetupView: View {
    @Binding var appSettings: AppSettings
    @State private var showSetupSheet = false
    @State private var keyboardStatus = KeyboardExtensionStatus.current
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        List {
            Section {
                VStack(spacing: 20) {
                    PhoneFrame(
                        imageName: "OnboardingKeyboard",
                        edge: .bottom,
                        verticalAnchor: 1,
                        framedEdgeInset: 0
                    )
                    .frame(height: 280)

                    VStack(spacing: 10) {
                        Text("使用键盘粘贴")
                            .font(.title2.weight(.bold))
                        Text("将复制的内容准备好，让您在任何输入地方粘贴")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                            .lineSpacing(2)
                    }

                    Button {
                        keyboardStatus = .current
                        showSetupSheet = true
                    } label: {
                        Text("启用键盘扩展")
                            .font(.body.weight(.semibold))
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .foregroundStyle(Color(.systemBackground))
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                }
                .padding(.vertical, 8)
            }

            Section {
                Toggle(isOn: $appSettings.keyboardSoundFeedback) {
                    Label("按键音", systemImage: "speaker.wave.2")
                }
                Toggle(isOn: $appSettings.keyboardHapticFeedback) {
                    Label("触感反馈", systemImage: "hand.tap")
                }
            } header: {
                Text("按键反馈")
            } footer: {
                Text("在 UniClip 键盘上点按时的声音与振动。按键音还受系统「设置 › 声音与触感」里「键盘点击音」总开关影响;触感反馈需要键盘「允许完全访问」。")
                    .font(.caption)
            }
        }
        .navigationTitle("键盘扩展")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            keyboardStatus = .current
            if ProcessInfo.processInfo.environment["UC_OPEN_KEYBOARD_SHEET"] == "1" {
                showSetupSheet = true
            }
        }
        .onChange(of: scenePhase) { _, newPhase in
            if newPhase == .active { keyboardStatus = .current }
        }
        .sheet(isPresented: $showSetupSheet) {
            KeyboardSetupSheet(status: keyboardStatus)
        }
    }
}

#Preview {
    @Previewable @State var settings = AppSettings.defaults
    return NavigationStack {
        KeyboardSetupView(appSettings: $settings)
    }
}
