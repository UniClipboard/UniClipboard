import SwiftUI

/// Confirmation sheet shown when a `uniclipboard://connect?…` URI arrives
/// via `.onOpenURL` and the user already has at least one server
/// configured. `SetupFlowView` (the no-server branch) consumes the same
/// `vm.pendingImport` first via its own `.task(id:)` and pushes the
/// prefilled form, so this sheet only ever appears on the main tabs.
///
/// Password is shown masked because the typical use case is scanning a
/// QR in a public place (mobile-sync pairing on a desktop next to you);
/// nobody needs to read it back, but it's useful to confirm something
/// landed.
struct ConnectImportSheet: View {
    let payload: ConnectURI.Payload
    var onConfirm: () -> Void
    var onCancel: () -> Void

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("扫描到 UniClipboard 配对二维码,确认添加此服务器吗?")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }

                Section("服务器") {
                    if let label = payload.label?.trimmingCharacters(in: .whitespacesAndNewlines),
                       !label.isEmpty {
                        LabeledContent("名称", value: label)
                    }
                    LabeledContent("地址", value: payload.url)
                    LabeledContent("用户名", value: payload.user)
                    LabeledContent("密码", value: String(repeating: "•", count: max(8, payload.pwd.count)))
                }

                if let device = payload.deviceId, !device.isEmpty {
                    Section("来源") {
                        LabeledContent("设备", value: device)
                    }
                }
            }
            .navigationTitle("添加服务器")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        onCancel()
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("添加") {
                        onConfirm()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
    }
}

/// Maps a `ConnectURI.ParseError` to a single localized sentence shown in
/// the root alert. Format arguments are passed via `String(localized:)`
/// interpolation — the catalog stores the zh-Hans source key.
func connectURIErrorMessage(_ error: ConnectURI.ParseError) -> String {
    switch error {
    case .invalidScheme:
        return String(localized: "这个二维码不属于 UniClipboard。")
    case .unsupportedVersion(let found):
        return String(localized: "二维码版本是 v\(found),本 App 暂不支持,请更新 App。")
    case .unsupportedService(let found):
        return String(localized: "二维码声明的服务 \"\(found)\" 不是手机同步。")
    case .payloadDecodeFailed:
        return String(localized: "二维码内容损坏,请在桌面端重新生成。")
    case .missingField(let name):
        return String(localized: "二维码缺少必需字段 \"\(name)\",请在桌面端重新生成。")
    case .invalidURL:
        return String(localized: "二维码里的服务地址不合法。")
    }
}

#Preview("Sheet") {
    Color.gray.opacity(0.2)
        .ignoresSafeArea()
        .sheet(isPresented: .constant(true)) {
            ConnectImportSheet(
                payload: ConnectURI.Payload(
                    url: "http://192.168.1.5:42720",
                    user: "mobile_aabbccdd",
                    pwd: "AbCdEfGhIjKlMnOpQrSt",
                    other: ["did": "did_0123abcd", "label": "Office Mac", "proto": "syncclipboard"]
                ),
                onConfirm: {},
                onCancel: {}
            )
            .presentationDetents([.medium, .large])
        }
}
