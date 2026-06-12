import SwiftUI

/// First-run setup. Drives the user from "no server configured" to a saved
/// active ServerConfig in two steps: Welcome → ServerForm (the form saves
/// directly — URL auto-switch needs no per-config setup anymore, it's
/// derived from the candidate URLs themselves, §5.3).
///
/// Env-driven shortcuts (debug / screenshots only):
/// - `UC_SETUP_STEP=form`        → start on ServerForm
/// - `UC_PREFILL=1`              → prefill the form with reasonable defaults
/// - `UC_PREFILL_TEST=success|authFailed|unreachable|missingFields`
///                                → seed the test-connection result
struct SetupFlowView: View {
    @Bindable var vm: AppViewModel
    var onComplete: () -> Void

    @State private var path: [Step] = Step.initialPath()

    /// Alias for the server being configured. Hoisted up here (rather than
    /// inside `ServerFormStepView`) so popping back to the form preserves
    /// what the user typed — destinations get torn down on pop.
    @State private var draftName: String = ""

    private var existingNames: Set<String> {
        Set(vm.servers.configs.compactMap(\.name))
    }

    enum Step: Hashable {
        case form
        /// Form with the credential fields pre-filled from a
        /// `uniclipboard://connect?…` URI. The shape is the same as
        /// `.form`; carrying the values through the path makes the step
        /// poppable + restorable (NavigationStack tears down destinations
        /// on pop, so `@State` defaults inside `ServerFormStepView` wouldn't
        /// survive a back-and-forth). `urls` is the full §5.6 candidate
        /// list (never empty; single-URL QRs yield one element).
        case formPrefilled(urls: [String], username: String, password: String)

        static func initialPath() -> [Step] {
            switch ProcessInfo.processInfo.environment["UC_SETUP_STEP"] {
            case "form":
                return [.form]
            default:
                return []
            }
        }
    }

    var body: some View {
        NavigationStack(path: $path) {
            WelcomeStepView(onScanned: handleScanned(_:))
                .navigationDestination(for: Step.self) { step in
                    switch step {
                    case .form:
                        ServerFormStepView(
                            name: $draftName,
                            existingNames: existingNames,
                            servers: $vm.servers,
                            ssidProvider: vm.ssidProvider,
                            onComplete: onComplete
                        )
                    case .formPrefilled(let urls, let username, let password):
                        ServerFormStepView(
                            name: $draftName,
                            existingNames: existingNames,
                            servers: $vm.servers,
                            ssidProvider: vm.ssidProvider,
                            onComplete: onComplete,
                            initialURLs: urls,
                            initialUsername: username,
                            initialPassword: password
                        )
                    }
                }
        }
        .task(id: vm.pendingImport) {
            // QR landed via `.onOpenURL` (system Camera) while still in
            // setup. Replace the nav path with the prefilled form — the
            // user came from outside the app and shouldn't have to retrace
            // Welcome → Form by hand. Seed `draftName` from the optional
            // label so the alias field isn't empty after navigation.
            guard let p = vm.consumePendingImport() else { return }
            draftName = p.label?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            path = [.formPrefilled(urls: p.urls, username: p.user, password: p.pwd)]
        }
    }

    /// Shared landing point for both the Welcome in-app QR scanner and the
    /// external `.onOpenURL` route — push the prefilled form, seed the
    /// alias from the optional label. `ServerQRPayload` is the common
    /// shape (ConnectURI's label is mapped to `name` upstream).
    private func handleScanned(_ payload: ServerQRPayload) {
        draftName = payload.name?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        path = [.formPrefilled(
            urls: payload.effectiveURLs,
            username: payload.username,
            password: payload.password
        )]
    }
}

// MARK: - Step 1 · Welcome

private struct WelcomeStepView: View {
    var onScanned: (ServerQRPayload) -> Void

    @Environment(\.openURL) private var openURL
    @State private var scannerPresented: Bool =
        ProcessInfo.processInfo.environment["UC_OPEN_SETUP_QR"] == "1"

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [Color.accentColor.opacity(0.10), .clear],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            VStack(spacing: 0) {
                Spacer()

                ZStack {
                    Circle()
                        .fill(Color.accentColor.opacity(0.12))
                        .frame(width: 168, height: 168)
                    Image(systemName: "qrcode.viewfinder")
                        .font(.system(size: 72, weight: .semibold))
                        .foregroundStyle(.tint)
                }
                .padding(.bottom, 36)

                VStack(spacing: 14) {
                    Text("和其他设备共享剪贴板")
                        .font(.largeTitle.weight(.bold))
                        .multilineTextAlignment(.center)
                    Text("扫描桌面端的配对二维码,几秒钟完成连接")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                .padding(.horizontal, 8)

                Spacer()

                VStack(spacing: 12) {
                    Button {
                        scannerPresented = true
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "qrcode.viewfinder")
                            Text("扫描二维码添加")
                                .font(.body.weight(.semibold))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        // AccentColor is near-black in light mode but
                        // ivory in dark mode, so the default white
                        // borderedProminent label gives no contrast on
                        // dark. systemBackground flips with appearance
                        // and lands on the correct contrasting tone.
                        .foregroundStyle(Color(.systemBackground))
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)

                    NavigationLink(value: SetupFlowView.Step.form) {
                        Text("手动输入服务器信息")
                            .font(.subheadline.weight(.medium))
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 10)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.large)

                    Button {
                        if let url = URL(string: "https://github.com/UniClipboard/UniClipboard") {
                            openURL(url)
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "questionmark.circle")
                            Text("我还没有服务器")
                        }
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    }
                    .padding(.top, 4)
                }
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 32)
        }
        .navigationBarHidden(true)
        .fullScreenCover(isPresented: $scannerPresented) {
            QRScannerView(
                onScan: { payload in
                    scannerPresented = false
                    // Defer path replacement by one runloop tick so the
                    // fullScreenCover finishes dismissing — stacking the
                    // NavigationStack mutation on the same tick swallows
                    // the transition and leaves the cover half-dismissed.
                    DispatchQueue.main.async {
                        onScanned(payload)
                    }
                },
                onCancel: { scannerPresented = false }
            )
        }
    }
}


// MARK: - Step 2 · ServerForm (final step — saves directly)

private struct ServerFormStepView: View {
    @Binding var name: String
    let existingNames: Set<String>
    @Binding var servers: ServerConfigList
    let ssidProvider: CurrentSSIDProvider
    var onComplete: () -> Void

    /// Raw candidate-URL editor rows (§5.1); cleaned on save.
    @State private var urls: [String]
    @State private var username: String
    @State private var password: String
    @State private var trustInsecure: Bool = false
    /// Latest "测试连接" verdict; nil = never tested, or invalidated by an
    /// edit (the probe section reports both through `onProbed`). Gates
    /// "完成" — first-run must not save a config that was never reached.
    @State private var probeSummary: ProbeSummary?
    /// Env-seeded verdict for screenshot recipes; see `SetupPrefill`.
    private let probeSeed: ProbeSummary?

    /// - Parameters:
    ///   - initialURLs/Username/Password: when non-nil, seed the form with
    ///     these values and skip the `UC_PREFILL` env defaults. Used by
    ///     `Step.formPrefilled` so a connect-URI scan from outside the app
    ///     lands the user on a ready-to-test form. The probe state stays
    ///     empty so the user explicitly presses "测试连接" — we don't want
    ///     a stale ✓ implying the QR credentials were already verified.
    init(
        name: Binding<String>,
        existingNames: Set<String>,
        servers: Binding<ServerConfigList>,
        ssidProvider: CurrentSSIDProvider,
        onComplete: @escaping () -> Void,
        initialURLs: [String]? = nil,
        initialUsername: String? = nil,
        initialPassword: String? = nil
    ) {
        _name = name
        self.existingNames = existingNames
        _servers = servers
        self.ssidProvider = ssidProvider
        self.onComplete = onComplete
        let prefillURL = SetupPrefill.url
        let seededURLs = initialURLs ?? (prefillURL.isEmpty ? [""] : [prefillURL])
        _urls = State(initialValue: seededURLs)
        _username = State(initialValue: initialUsername ?? SetupPrefill.username)
        _password = State(initialValue: initialPassword ?? SetupPrefill.password)
        // Env-driven screenshot modes (UC_PREFILL_TEST=success/…) need a
        // verdict already on screen without a tappable button. Real QR
        // prefill (initialURLs != nil) never seeds — see the doc comment.
        if initialURLs == nil,
           let result = SetupPrefill.initialProbeResult,
           let target = seededURLs.first, !target.isEmpty {
            let seed = ProbeSummary(
                results: [target: result],
                picked: result.isReachable ? target : nil
            )
            probeSeed = seed
            _probeSummary = State(initialValue: seed)
        } else {
            probeSeed = nil
        }
    }

    var body: some View {
        Form {
            Section {
                HStack(spacing: 8) {
                    TextField("便于辨识的名称", text: $name)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button {
                        name = ServerNameGenerator.generate(avoiding: existingNames)
                    } label: {
                        Image(systemName: "shuffle")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("换一个名称")
                }
            } header: {
                Text("名称")
            } footer: {
                Text("将显示在剪贴板顶栏。留空会用服务器地址替代。")
            }

            ServerURLsEditorSection(
                urls: $urls,
                trustInsecureCert: $trustInsecure
            )

            Section("凭据") {
                TextField("用户名", text: $username)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                SecureField("密码", text: $password)
            }

            TestAllConnectionsSection(
                urls: urls,
                username: username,
                password: password,
                trustInsecureCert: trustInsecure,
                network: ssidProvider.networkContext,
                onProbed: { probeSummary = $0 },
                seed: probeSeed
            )

            // "完成" sits in its own Section so the test action and the
            // commit action aren't visually coupled. Rendered always —
            // disabled until a passing test — so the path forward is
            // visible from the moment the form opens.
            Section {
                Button {
                    save()
                } label: {
                    HStack {
                        Text("完成")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(.tint)
                        Spacer()
                    }
                }
                .disabled(probeSummary?.hasSuccess != true)
            } footer: {
                if let s = probeSummary, s.picked == nil {
                    permissionHintFooter
                }
            }
        }
        .navigationTitle("服务器配置")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            // First visit: seed with a fresh alias. If the user clears it
            // intentionally we don't refill — `.isEmpty` will be saved as
            // `nil` and `displayLabel` falls back to URL per §5.1.
            if name.isEmpty {
                name = ServerNameGenerator.generate(avoiding: existingNames)
            }
            // Why not auto-fire the probe on QR landing: iOS Local Network
            // permission is request-on-first-use, and the first attempted
            // request *always* fails (the system prompts the user in
            // parallel; the URLSession returns an error immediately, not
            // after the dialog resolves). Auto-firing would mean the user
            // taps Allow and still sees "无法连接" — confusing. So we let
            // the user press "测试连接" themselves; that press is also what
            // triggers the OS prompt the first time, with clear cause.
        }
    }

    @ViewBuilder
    private var permissionHintFooter: some View {
        Button {
            if let url = URL(string: UIApplication.openSettingsURLString) {
                UIApplication.shared.open(url)
            }
        } label: {
            (Text("如果刚才拒绝了本地网络权限,")
                + Text("前往设置 ›").foregroundColor(.accentColor))
                .font(.footnote)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(.plain)
    }

    private func save() {
        var seen = Set<String>()
        let cleaned = urls
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty && seen.insert($0).inserted }
        guard !cleaned.isEmpty else { return }
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let server = ServerConfig(
            id: UUID().uuidString.lowercased(),
            name: trimmedName.isEmpty ? nil : trimmedName,
            urls: cleaned,
            username: username,
            password: password
        )
        servers.configs.append(server)
        servers.activeConfigId = server.id
        onComplete()
    }
}

// MARK: - Prefill (env-driven for screenshots / quick demo)

private enum SetupPrefill {
    static var name:     String { value("UC_PREFILL_NAME",     fallback: "happy-otter") }
    static var url:      String { value("UC_PREFILL_URL",      fallback: "https://clip.home.lan:5033/") }
    static var username: String { value("UC_PREFILL_USERNAME", fallback: "alice") }
    static var password: String { value("UC_PREFILL_PASSWORD", fallback: "p4ssw0rd!") }

    /// `UC_PREFILL_TEST` → a pre-baked probe verdict for the prefilled URL,
    /// so screenshot recipes can show the tested form without a tap.
    static var initialProbeResult: ConnectionTester.Result? {
        switch ProcessInfo.processInfo.environment["UC_PREFILL_TEST"] {
        case "success":       return .success
        case "authFailed":    return .authFailed
        case "unreachable":   return .unreachable
        case "missingFields": return .missingFields
        default:              return nil
        }
    }

    private static func value(_ key: String, fallback: String) -> String {
        let env = ProcessInfo.processInfo.environment
        guard env["UC_PREFILL"] == "1" else { return "" }
        return env[key] ?? fallback
    }
}

#Preview("Welcome") {
    SetupFlowView(vm: .preview(servers: ServerConfigList())) {}
}
