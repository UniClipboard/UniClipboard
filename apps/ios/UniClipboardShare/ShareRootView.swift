import SwiftUI
import UIKit
import OSLog
import SentryWithoutUIKit

private let log = Logger(subsystem: "app.uniclipboard", category: "share")

/// The Share Extension's only screen. Loads the shared payload, lets the
/// user confirm which server to push to (if more than one is configured),
/// runs the upload, then dismisses. The whole flow is asynchronous; the
/// state machine below is what keeps the SwiftUI side honest.
@MainActor
struct ShareRootView: View {
    let context: ShareExtensionContext?
    /// Server id pre-selected by iOS when the user tapped a Sharing
    /// Suggestions tile. When non-nil and matching a known server, we
    /// skip the picker UI and go straight to upload (`.uploading`)
    /// the moment attachment loading finishes. When non-nil but stale
    /// (server was deleted), we fall back to the picker and surface a
    /// note so the user knows why the shortcut didn't fire.
    var prefilledServerId: String? = nil
    let onFinish: () -> Void
    let onCancel: () -> Void

    @State private var phase: Phase = .loadingAttachment
    @State private var item: ShareItem?
    @State private var servers: ServerConfigList = ServerConfigList()
    @State private var trustInsecureCert: Bool = false
    @State private var selectedServerId: String?
    @State private var prefillNote: String? = nil
    /// Mirrors the user's appearance setting from the App Group so the
    /// share sheet matches the main app instead of always rendering in
    /// whatever the system happens to be set to.
    @State private var appearance: AppearanceMode = .system

    enum Phase: Equatable {
        case loadingAttachment
        case ready
        case uploading
        case succeeded
        case failed(String)
    }

    var body: some View {
        NavigationStack {
            Group {
                switch phase {
                case .loadingAttachment:
                    centered { ProgressView(String(localized: "正在读取分享内容…")) }
                case .ready:
                    readyForm
                case .uploading:
                    centered {
                        VStack(spacing: 12) {
                            ProgressView()
                            Text("正在发送到 \(selectedServerName)…")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }
                case .succeeded:
                    centered {
                        VStack(spacing: 12) {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.largeTitle)
                                .foregroundStyle(.green)
                            Text("已发送")
                                .font(.headline)
                        }
                    }
                case .failed(let msg):
                    centered {
                        VStack(spacing: 12) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.largeTitle)
                                .foregroundStyle(.orange)
                            Text("发送失败")
                                .font(.headline)
                            Text(msg)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .multilineTextAlignment(.center)
                                .padding(.horizontal, 24)
                        }
                    }
                }
            }
            .navigationTitle("分享到 UniClipboard")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("取消") { onCancel() }
                        .disabled(phase == .uploading)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    trailingButton
                }
            }
        }
        .task { await loadEverything() }
        .preferredColorScheme(appearance.colorScheme)
    }

    // MARK: - Subviews

    @ViewBuilder
    private var readyForm: some View {
        Form {
            if let note = prefillNote {
                Section {
                    Label(note, systemImage: "info.circle")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            Section(String(localized: "内容")) {
                if let item {
                    contentRow(for: item)
                }
            }

            if servers.configs.count > 1 {
                Section(String(localized: "发送到")) {
                    ForEach(servers.configs, id: \.id) { server in
                        Button {
                            selectedServerId = server.id
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(server.displayLabel)
                                        .foregroundStyle(.primary)
                                    if server.name?.isEmpty == false {
                                        Text(server.url)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }
                                }
                                Spacer()
                                if server.id == selectedServerId {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                }
            } else if let only = servers.configs.first {
                Section(String(localized: "发送到")) {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(only.displayLabel)
                            if only.name?.isEmpty == false {
                                Text(only.url)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        Spacer()
                    }
                }
            } else {
                Section {
                    Text("尚未配置服务器,请先打开 UniClipboard 主程序添加")
                        .foregroundStyle(.secondary)
                        .font(.footnote)
                }
            }
        }
    }

    @ViewBuilder
    private func contentRow(for item: ShareItem) -> some View {
        HStack(alignment: .top, spacing: 12) {
            iconView(for: item)
                .frame(width: 44, height: 44)
            VStack(alignment: .leading, spacing: 4) {
                Text(item.displayName)
                    .font(.callout)
                    .lineLimit(3)
                Text(sizeLabel(for: item))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func iconView(for item: ShareItem) -> some View {
        switch item {
        case .text:
            iconBadge(systemName: "doc.text", tint: .blue)
        case .image(let bytes, _):
            if let uiImage = UIImage(data: bytes) {
                Image(uiImage: uiImage)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 44, height: 44)
                    .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            } else {
                iconBadge(systemName: "photo", tint: .pink)
            }
        case .file:
            iconBadge(systemName: "doc", tint: .orange)
        }
    }

    private func iconBadge(systemName: String, tint: Color) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(tint.opacity(0.15))
            Image(systemName: systemName)
                .foregroundStyle(tint)
        }
    }

    private func sizeLabel(for item: ShareItem) -> String {
        switch item {
        case .text(let s):
            return String(localized: "文本 · \(s.count) 字")
        case .image(let b, let ext):
            let f = ByteCountFormatter()
            return String(localized: "图片 · \(ext.uppercased()) · \(f.string(fromByteCount: Int64(b.count)))")
        case .file(_, let b):
            let f = ByteCountFormatter()
            return String(localized: "文件 · \(f.string(fromByteCount: Int64(b.count)))")
        }
    }

    @ViewBuilder
    private var trailingButton: some View {
        switch phase {
        case .succeeded, .failed:
            Button("完成") { onFinish() }
                .bold()
        case .ready:
            Button("发送") { Task { await send() } }
                .bold()
                .disabled(!canSend)
        default:
            EmptyView()
        }
    }

    // MARK: - Helpers

    private var canSend: Bool {
        item != nil && resolvedServer != nil
    }

    private var resolvedServer: ServerConfig? {
        if let id = selectedServerId {
            return servers.configs.first(where: { $0.id == id })
        }
        return servers.activeConfig
    }

    private var selectedServerName: String {
        if let s = resolvedServer { return s.displayLabel }
        return String(localized: "未选择服务器")
    }

    private func centered<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        VStack {
            Spacer()
            content()
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Actions

    private func loadEverything() async {
        let store = SettingsStore()
        let loadedServers = store.loadServers()
        let loadedSettings = store.loadAppSettings()
        servers = loadedServers
        trustInsecureCert = loadedSettings.trustInsecureCert
        appearance = loadedSettings.appearance

        // Resolve initial selection: Sharing-Suggestions tap takes
        // priority; if the tapped server has since been deleted we note
        // it and fall back to the user's active server / first server.
        if let pre = prefilledServerId {
            if loadedServers.configs.contains(where: { $0.id == pre }) {
                selectedServerId = pre
            } else {
                selectedServerId = loadedServers.activeConfigId ?? loadedServers.configs.first?.id
                prefillNote = String(localized: "原服务器已不可用,已切换到当前活动服务器")
            }
        } else {
            selectedServerId = loadedServers.activeConfigId ?? loadedServers.configs.first?.id
        }

        guard let ctx = context else {
            log.error("loadEverything: no extension context")
            phase = .failed(String(localized: "没有可分享的内容"))
            return
        }
        do {
            let extracted = try await ShareItemExtractor.extract(from: ctx)
            log.info("loadEverything: extracted \(extracted.kindLabel, privacy: .public) bytes=\(extracted.byteCount, privacy: .public)")
            item = extracted
            // Direct-share fast path: the user already told iOS which
            // server to use, so skip the picker entirely. `send()` sets
            // `.uploading` itself.
            if prefilledServerId != nil, prefillNote == nil, resolvedServer != nil {
                await send()
            } else {
                phase = .ready
            }
        } catch {
            // The activation rule matched but extraction failed — the
            // source app advertised a UTI it couldn't fulfill, or our
            // extractor has a gap. Bug-grade either way.
            log.error("loadEverything: extraction failed: \(String(describing: error), privacy: .public)")
            SentrySDK.logger.error(
                "share extraction failed",
                attributes: ["error": String(describing: error)]
            )
            phase = .failed((error as? LocalizedError)?.errorDescription ?? "\(error)")
        }
    }

    private func send() async {
        guard let item, var server = resolvedServer else { return }
        // §5.3 from an extension: like the keyboard, the share sheet never
        // probes. Layer the main app's last probe verdict (App Group
        // `live_urls`) over pure shape order so the upload PUTs against the
        // path that actually works on this network instead of burning a
        // timeout on `urls[0]`. No NWPathMonitor here (the sheet lives for
        // seconds) — the stored SSID stands in as the Wi-Fi signal and
        // Tailscale is checked live, both entitlement-free.
        let store = SettingsStore()
        server.urls = server.preferredURLs(
            live: store.loadLiveURL(configId: server.id),
            network: NetworkContext(
                ssid: store.loadLastKnownSSID(),
                isTailscale: TailscaleDetector.isActive()
            )
        )
        phase = .uploading
        do {
            let uploader = ShareUploader()
            try await uploader.upload(item, to: server, trustInsecureCert: trustInsecureCert)
            log.info("send: upload succeeded \(item.kindLabel, privacy: .public) bytes=\(item.byteCount, privacy: .public)")
            SentrySDK.logger.info(
                "share upload succeeded",
                attributes: ["kind": item.kindLabel, "bytes": item.byteCount]
            )
            phase = .succeeded
        } catch {
            let kind = (error as? SyncError).map { String(describing: $0.kind) } ?? String(describing: type(of: error))
            log.error("send: upload failed \(kind, privacy: .public): \(String(describing: error), privacy: .private)")
            SentrySDK.logger.error(
                "share upload failed",
                attributes: ["errorKind": kind, "kind": item.kindLabel, "bytes": item.byteCount]
            )
            let msg = (error as? LocalizedError)?.errorDescription ?? "\(error)"
            phase = .failed(msg)
        }
    }
}
