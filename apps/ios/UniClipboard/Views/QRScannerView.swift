import SwiftUI
import Vision
import VisionKit

/// Payload decoded from a UniClipboard server-config QR code.
///
/// Accepted formats (in priority order):
/// - Connect URI: `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url>`
///   — the canonical format minted by the desktop's mobile-sync pairing
///   menu. Parsed by `ConnectURI`; the optional `o.label` is mapped to
///   `name` so the SetupFlow seeds a human-friendly alias.
/// - JSON object: `{"url":"…","username":"…","password":"…","name":"…?"}`
///   — `name` is optional. Extra fields are tolerated for forward
///   compatibility (decoder ignores them).
/// - URL with userinfo: `https://user:pass@host:port/` — `name` is nil,
///   `url` is reconstructed without the userinfo segment.
///
/// Anything else fails parsing; the scanner shows an inline alert and keeps
/// scanning. A malformed connect URI does **not** fall through to the
/// legacy paths — it's still a UniClipboard QR, just a broken one, and
/// pretending otherwise would hide the bug.
struct ServerQRPayload: Codable, Equatable, Sendable {
    var name: String?
    var url: String
    /// §5.6 ordered candidate list (`urls[0] == url`). Nil/empty when the
    /// QR predates multi-URL; read through `effectiveURLs`.
    var urls: [String]? = nil
    var username: String
    var password: String

    /// The candidate list to provision, never empty: `urls` when the QR
    /// carried one, else the single legacy `url`.
    var effectiveURLs: [String] {
        if let urls, !urls.isEmpty { return urls }
        return [url]
    }

    static func parse(_ raw: String) -> ServerQRPayload? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        // Primary path: connect URI from the desktop.
        if trimmed.lowercased().hasPrefix("uniclipboard://") {
            guard let p = try? ConnectURI.parse(trimmed) else { return nil }
            return ServerQRPayload(
                name: p.label, url: p.url, urls: p.urls, username: p.user, password: p.pwd
            )
        }

        // JSON fallback — older desktops / hand-written QRs.
        if let data = trimmed.data(using: .utf8),
           let payload = try? JSONDecoder().decode(ServerQRPayload.self, from: data),
           !payload.url.isEmpty,
           !payload.username.isEmpty,
           !payload.password.isEmpty {
            return payload
        }

        // URL-with-userinfo fallback. Reject when userinfo is missing — a
        // bare URL without credentials isn't enough to provision a server.
        guard let url = URL(string: trimmed),
              let host = url.host, !host.isEmpty,
              let user = url.user?.removingPercentEncoding, !user.isEmpty,
              let pass = url.password?.removingPercentEncoding, !pass.isEmpty
        else { return nil }

        var clean = URLComponents()
        clean.scheme = url.scheme ?? "https"
        clean.host = host
        clean.port = url.port
        clean.path = url.path.isEmpty ? "/" : url.path
        guard let cleanString = clean.string else { return nil }
        return ServerQRPayload(name: nil, url: cleanString, username: user, password: pass)
    }
}

/// Camera-based QR-code scanner that yields a `ServerQRPayload`. Presents
/// itself as a full-screen cover; the caller handles dismissal via the
/// callbacks.
///
/// Simulator / unsupported device: shows a static "unavailable" panel and
/// honors `UC_TEST_QR_PAYLOAD` (env-driven payload injection for screenshot
/// recipes — see CLAUDE.md "Launch-time env hooks").
struct QRScannerView: View {
    var onScan: (ServerQRPayload) -> Void
    var onCancel: () -> Void

    @State private var alertMessage: String?

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            scannerCore
            chrome
        }
        // Don't set .preferredColorScheme(.dark) here — SwiftUI leaks the
        // preference into the next presented view after the fullScreenCover
        // dismisses, which turns the Add Server sheet dark unexpectedly.
        // The scanner content above is already styled with explicit white
        // foreground colors, so the dark backdrop is enough.
        .alert(
            "无法识别该二维码",
            isPresented: Binding(
                get: { alertMessage != nil },
                set: { if !$0 { alertMessage = nil } }
            )
        ) {
            Button("好") { alertMessage = nil }
        } message: {
            if let alertMessage { Text(alertMessage) }
        }
        .task {
            // Screenshot / preview hook: feed a payload as if just scanned.
            if let raw = ProcessInfo.processInfo.environment["UC_TEST_QR_PAYLOAD"] {
                handle(raw: raw)
            }
        }
    }

    @ViewBuilder
    private var scannerCore: some View {
        if DataScannerViewController.isSupported && DataScannerViewController.isAvailable {
            DataScannerWrapper(onScan: handle(raw:))
                .ignoresSafeArea()
                .overlay(reticle)
        } else {
            unavailablePanel
        }
    }

    private var reticle: some View {
        // A simple centered square frame so users know where to aim.
        RoundedRectangle(cornerRadius: 18)
            .stroke(Color.white.opacity(0.85), lineWidth: 2)
            .frame(width: 240, height: 240)
            .shadow(color: .black.opacity(0.6), radius: 8)
    }

    private var unavailablePanel: some View {
        VStack(spacing: 12) {
            Image(systemName: "camera.fill")
                .font(.largeTitle)
                .foregroundStyle(.white.opacity(0.5))
            Text("当前设备无法扫描二维码")
                .foregroundStyle(.white)
                .font(.subheadline.weight(.semibold))
            Text("可能没有相机权限,或正在模拟器中运行")
                .foregroundStyle(.white.opacity(0.6))
                .font(.caption)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var chrome: some View {
        VStack {
            HStack {
                Button {
                    onCancel()
                } label: {
                    Text("取消")
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.vertical, 8)
                        .padding(.horizontal, 14)
                        .background(.ultraThinMaterial, in: Capsule())
                }
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.top, 8)

            Spacer()

            Text("将服务器二维码对准框内")
                .font(.subheadline)
                .foregroundStyle(.white)
                .padding(.vertical, 10)
                .padding(.horizontal, 16)
                .background(.black.opacity(0.45), in: Capsule())
                .padding(.bottom, 48)
        }
    }

    private func handle(raw: String) {
        if let payload = ServerQRPayload.parse(raw) {
            onScan(payload)
        } else {
            alertMessage = "二维码内容不是 UniClipboard 服务器配置"
        }
    }
}

/// Thin `UIViewControllerRepresentable` over `DataScannerViewController`.
/// The wrapper starts scanning on first `updateUIViewController` and fires
/// `onScan` exactly once — repeated barcode adds during the same lifetime
/// are ignored so the parent can dismiss without races.
private struct DataScannerWrapper: UIViewControllerRepresentable {
    var onScan: (String) -> Void

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let vc = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isHighlightingEnabled: true
        )
        vc.delegate = context.coordinator
        return vc
    }

    func updateUIViewController(_ vc: DataScannerViewController, context: Context) {
        if !vc.isScanning {
            try? vc.startScanning()
        }
    }

    static func dismantleUIViewController(_ vc: DataScannerViewController, coordinator: Coordinator) {
        vc.stopScanning()
    }

    func makeCoordinator() -> Coordinator { Coordinator(onScan: onScan) }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let onScan: (String) -> Void
        private var fired = false

        init(onScan: @escaping (String) -> Void) {
            self.onScan = onScan
        }

        func dataScanner(
            _ scanner: DataScannerViewController,
            didAdd addedItems: [RecognizedItem],
            allItems: [RecognizedItem]
        ) {
            guard !fired else { return }
            for item in addedItems {
                if case .barcode(let barcode) = item,
                   let raw = barcode.payloadStringValue {
                    fired = true
                    onScan(raw)
                    return
                }
            }
        }
    }
}
