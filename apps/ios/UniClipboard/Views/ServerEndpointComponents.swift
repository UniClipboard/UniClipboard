import SwiftUI

// MARK: - §5.3 URL class presentation

extension ServerURLClass {
    /// Localized human label for the host-shape class.
    var displayName: String {
        switch self {
        case .lan:       String(localized: "局域网")
        // Brand name — verbatim, not a catalog key.
        case .tailscale: "Tailscale"
        case .wan:       String(localized: "公网")
        }
    }

    var iconName: String {
        switch self {
        case .lan:       "house.fill"
        case .tailscale: "point.3.connected.trianglepath.dotted"
        case .wan:       "globe"
        }
    }

    /// Display order for summary chips: direct paths first, relay last —
    /// mirrors the on-Wi-Fi §5.3 preference so the chips read as "best
    /// path available".
    static let displayOrder: [ServerURLClass] = [.lan, .tailscale, .wan]
}

/// Tiny capsule chip naming one URL's host-shape class. Used as the
/// trailing accessory on URL rows in the editor and the probe list.
struct ServerURLClassChip: View {
    let urlClass: ServerURLClass

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: urlClass.iconName)
            Text(urlClass.displayName)
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(Color(.tertiarySystemFill), in: Capsule(style: .continuous))
    }
}

/// One-line summary of a profile's candidate classes, for server-list rows
/// (Settings → 服务器列表 and the Home switcher sheet). Replaces the old
/// per-config auto-switch badge: routing is URL-shape-based now, so the
/// glanceable fact is *which kinds of path* this profile carries. Renders
/// nothing for single-URL profiles — the URL line above it already says
/// everything.
struct ServerURLClassSummary: View {
    let urls: [String]

    var body: some View {
        if urls.count > 1 {
            HStack(spacing: 4) {
                Text("\(urls.count) 个地址")
                ForEach(presentClasses, id: \.self) { cls in
                    HStack(spacing: 2) {
                        Image(systemName: cls.iconName)
                        Text(cls.displayName)
                    }
                }
            }
            .font(.caption2)
            .foregroundStyle(.tertiary)
            .lineLimit(1)
        }
    }

    private var presentClasses: [ServerURLClass] {
        let present = Set(urls.map(ServerConfig.classifyURL))
        return ServerURLClass.displayOrder.filter(present.contains)
    }
}

// MARK: - Multi-URL editor section

/// Editable candidate-URL list (§5.1 `urls`) + the trust-insecure toggle,
/// shared by the Settings edit page, the add sheet, and the Setup form.
///
/// The binding holds raw editor rows — blanks included while the user
/// types. Callers filter/trim on commit (`ServerDraft.cleanedURLs` /
/// the edit view's write-back). Row order is the publisher order the
/// §5.3 sorter treats as tie-break, so first = default.
struct ServerURLsEditorSection: View {
    @Binding var urls: [String]
    @Binding var trustInsecureCert: Bool

    var body: some View {
        Section {
            ForEach(urls.indices, id: \.self) { i in
                HStack(spacing: 8) {
                    TextField("https://your-server.com:5033/", text: rowBinding(at: i))
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if let cls = rowClass(at: i) {
                        ServerURLClassChip(urlClass: cls)
                    }
                }
            }
            .onDelete { offsets in
                urls.remove(atOffsets: offsets)
                // Never present a zero-row editor — the form's save gate
                // needs at least one row to point the user at.
                if urls.isEmpty { urls = [""] }
            }
            Button {
                urls.append("")
            } label: {
                Label("添加备用地址", systemImage: "plus.circle")
            }
            Toggle(isOn: $trustInsecureCert) {
                Text("允许不安全证书")
            }
        } header: {
            Text("服务器地址")
        } footer: {
            Text("同一服务器可填多个地址（局域网 / Tailscale / 公网），App 会按当前网络自动选用可达的一条；第一条为默认地址。左滑删除。「允许不安全证书」仅在使用自签名 HTTPS 证书时需要，纯 HTTP 无需开启；此设置全局生效。")
        }
    }

    /// Index-guarded row binding: a swipe-delete can race a pending text
    /// commit from the just-removed row, and a raw `$urls[i]` would trap
    /// on the stale index.
    private func rowBinding(at i: Int) -> Binding<String> {
        Binding(
            get: { i < urls.count ? urls[i] : "" },
            set: { if i < urls.count { urls[i] = $0 } }
        )
    }

    private func rowClass(at i: Int) -> ServerURLClass? {
        guard i < urls.count else { return nil }
        let trimmed = urls[i].trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, URL(string: trimmed)?.host != nil else { return nil }
        return ServerConfig.classifyURL(trimmed)
    }
}

// MARK: - Multi-candidate connection test

/// Outcome of one "测试连接" pass over every candidate, handed to the host
/// form. `picked` is the §5.3 verdict — first reachable in shape order for
/// the network the probe ran on.
struct ProbeSummary: Equatable {
    var results: [String: ConnectionTester.Result]
    var picked: String?

    /// At least one candidate answered with working credentials — the
    /// Setup flow's "完成" gate.
    var hasSuccess: Bool { results.values.contains(.success) }
}

/// "测试连接" over the full candidate list: probes all URLs concurrently
/// (§5.3 Layer 2 semantics — 404/401 count as reachable), lists per-URL
/// reachability, and marks which candidate the current network will use.
///
/// Pure UI — persisting the verdict (seeding the live-URL cache) is the
/// host's job via `onProbed`, because only the host knows whether the
/// edited profile exists yet (the add sheet / Setup form probe drafts
/// that have no `configId` until saved).
struct TestAllConnectionsSection: View {
    /// Raw editor rows; blanks are filtered here so the host can pass its
    /// draft straight through.
    let urls: [String]
    let username: String
    let password: String
    let trustInsecureCert: Bool
    let network: NetworkContext
    /// Non-nil: a probe pass just finished. Nil: the user edited a
    /// probe-relevant field, invalidating the previous summary — hosts
    /// gating on it (Setup's "完成") must drop their cached copy.
    var onProbed: ((ProbeSummary?) -> Void)? = nil
    /// Pre-baked verdict installed on first appear — only for env-driven
    /// screenshot recipes (`UC_PREFILL_TEST`), which can't tap "测试连接".
    var seed: ProbeSummary? = nil

    @State private var results: [String: ConnectionTester.Result] = [:]
    @State private var orderedCandidates: [String] = []
    @State private var picked: String?
    @State private var isProbing = false
    @State private var hasProbed = false
    @State private var missingFields = false
    @State private var inflight: Task<Void, Never>?

    var body: some View {
        Section {
            if missingFields {
                Label {
                    Text("请填写地址、用户名和密码")
                } icon: {
                    Image(systemName: "exclamationmark.circle")
                        .foregroundStyle(.secondary)
                }
            }

            ForEach(orderedCandidates, id: \.self) { url in
                candidateRow(url)
            }

            if hasProbed, picked == nil {
                Label {
                    Text("全部不可达 — 检查地址和网络")
                } icon: {
                    Image(systemName: "wifi.exclamationmark")
                        .foregroundStyle(.orange)
                }
            }

            Button {
                runProbe()
            } label: {
                HStack {
                    if isProbing {
                        ProgressView().controlSize(.small)
                        Text("正在连接…")
                    } else {
                        Image(systemName: "bolt.horizontal.circle")
                        Text(hasProbed ? "重新测试" : "测试连接")
                    }
                    Spacer()
                }
            }
            .disabled(isProbing)
        } header: {
            Text("连接")
        } footer: {
            if hasProbed, candidates().count > 1 {
                Text("标注「将使用」的地址是当前网络下的首选；网络变化时会自动重选。")
                    .font(.caption)
            }
        }
        // Editing a probe-relevant field invalidates the last verdict.
        .onChange(of: urls) { _, _ in resetOnEdit() }
        .onChange(of: username) { _, _ in resetOnEdit() }
        .onChange(of: password) { _, _ in resetOnEdit() }
        .onChange(of: trustInsecureCert) { _, _ in resetOnEdit() }
        .onAppear {
            if let seed, !hasProbed, results.isEmpty {
                orderedCandidates = candidates().filter { seed.results[$0] != nil }
                results = seed.results
                picked = seed.picked
                hasProbed = true
            }
        }
    }

    @ViewBuilder
    private func candidateRow(_ url: String) -> some View {
        HStack(spacing: 8) {
            statusIcon(for: url)
            Text(url)
                .font(.footnote)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 4)
            ServerURLClassChip(urlClass: ServerConfig.classifyURL(url))
            if !isProbing, picked == url {
                Text("将使用")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor, in: Capsule(style: .continuous))
            }
        }
    }

    @ViewBuilder
    private func statusIcon(for url: String) -> some View {
        if isProbing {
            ProgressView().controlSize(.small)
        } else {
            switch results[url] {
            case .success:
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            case .authFailed:
                // Reachable path, broken credentials — the row is half
                // good news; the lock icon keeps the blame on the creds.
                Image(systemName: "lock.trianglebadge.exclamationmark.fill")
                    .foregroundStyle(.red)
            case .unreachable:
                Image(systemName: "xmark.circle")
                    .foregroundStyle(.orange)
            case .missingFields, nil:
                Image(systemName: "circle.dotted")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func candidates() -> [String] {
        var seen = Set<String>()
        return urls
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty && seen.insert($0).inserted }
    }

    private func runProbe() {
        inflight?.cancel()
        let candidates = candidates()
        guard !candidates.isEmpty, !username.isEmpty, !password.isEmpty else {
            missingFields = true
            orderedCandidates = []
            hasProbed = false
            return
        }
        missingFields = false
        // Display rows in the §5.3 try-order for the current network so the
        // list reads top-down as "what the app will attempt".
        let probeConfig = ServerConfig(
            id: "probe-ui", urls: candidates, username: username, password: password
        )
        let ordered = probeConfig.orderedURLs(network: network)
        orderedCandidates = ordered
        results = [:]
        picked = nil
        isProbing = true
        let user = username, pwd = password, trust = trustInsecureCert
        inflight = Task {
            let res = await ConnectionTester.probe(
                urls: candidates,
                username: user,
                password: pwd,
                trustInsecureCert: trust
            )
            guard !Task.isCancelled else { return }
            results = res
            picked = ConnectionTester.firstReachable(in: ordered, results: res)
            isProbing = false
            hasProbed = true
            onProbed?(ProbeSummary(results: res, picked: picked))
        }
    }

    private func resetOnEdit() {
        inflight?.cancel()
        isProbing = false
        let hadVerdict = hasProbed
        hasProbed = false
        missingFields = false
        results = [:]
        orderedCandidates = []
        picked = nil
        if hadVerdict { onProbed?(nil) }
    }
}
