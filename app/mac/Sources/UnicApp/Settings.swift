import SwiftUI
import SettingsSchema

/// Settings window, laid out like System Settings: a sidebar of panes, a grouped form for each.
enum SettingsPane: String, CaseIterable, Identifiable {
    case general, devices, plugins
    var id: String { rawValue }
    var title: String {
        switch self {
        case .general: return "General"
        case .devices: return "Devices"
        case .plugins: return "Plugins"
        }
    }
    var symbol: String {
        switch self {
        case .general: return "gearshape.fill"
        case .devices: return "laptopcomputer.and.iphone"
        case .plugins: return "puzzlepiece.extension.fill"
        }
    }
    var tint: Color {
        switch self {
        case .general: return .gray
        case .devices: return .blue
        case .plugins: return .purple
        }
    }
}

struct SettingsView: View {
    @EnvironmentObject var library: Library

    var body: some View {
        NavigationSplitView {
            List(selection: Binding(get: { library.settingsPane }, set: { if let p = $0 { library.settingsPane = p } })) {
                ForEach(SettingsPane.allCases) { pane in
                    Label { Text(pane.title) } icon: { PaneIcon(symbol: pane.symbol, tint: pane.tint) }
                        .badge(pane == .devices ? library.pending.count : 0)
                        .tag(pane)
                }
            }
            .listStyle(.sidebar)
            .navigationSplitViewColumnWidth(215)
            .toolbar(removing: .sidebarToggle)
        } detail: {
            switch library.settingsPane {
            case .general: GeneralPane()
            case .devices: DevicesPane()
            case .plugins: PluginsPane()
            }
        }
        .frame(width: 780, height: 540)
    }
}

/// Colored rounded-square icon, as in System Settings.
private struct PaneIcon: View {
    let symbol: String
    let tint: Color
    var body: some View {
        Image(systemName: symbol).font(.system(size: 13, weight: .medium)).foregroundStyle(.white)
            .frame(width: 24, height: 24)
            .background(tint.gradient, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}

/// Leading icon of a row inside a pane.
private struct RowIcon: View {
    let symbol: String
    var tint: Color = .blue
    var body: some View {
        Image(systemName: symbol).font(.system(size: 15, weight: .medium)).foregroundStyle(tint)
            .frame(width: 34, height: 34)
            .background(tint.opacity(0.15), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
    }
}

// MARK: - General

private struct GeneralPane: View {
    @EnvironmentObject var library: Library
    @AppStorage("launchAtLoginDemo") private var launch = true
    @AppStorage("retentionDemo") private var retention = "30 days"

    var body: some View {
        Form {
            Section {
                Toggle("Open at login", isOn: $launch)
                Toggle("Sync clipboard", isOn: Binding(get: { !library.paused }, set: { library.paused = !$0 }))
            }
            Section("History") {
                Picker("Keep history", selection: $retention) {
                    ForEach(["7 days", "30 days", "Forever"], id: \.self) { Text($0) }
                }
            }
            Section("Shortcuts") {
                LabeledContent("Quick Paste") { Text("⌥⌘V").foregroundStyle(.secondary) }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("General")
    }
}

// MARK: - Devices

private struct DevicesPane: View {
    @EnvironmentObject var library: Library
    @State private var adding = false
    @State private var removing: Device?

    var body: some View {
        Form {
            if !library.pending.isEmpty {
                Section {
                    ForEach(library.pending) { pendingRow($0) }
                } header: {
                    Text("Waiting for Approval")
                } footer: {
                    Text("These devices joined with a pairing code. Approve one only if its fingerprint matches the one shown on that device.")
                }
            }
            Section("Your Devices") {
                ForEach(library.devices) { deviceRow($0) }
            }
            Section {
                Button("Add Device…", systemImage: "plus") { adding = true }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("Devices")
        .sheet(isPresented: $adding) { AddDeviceSheet().environmentObject(library) }
        .confirmationDialog("Remove \(removing?.id ?? "device")?",
                            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
                            titleVisibility: .visible) {
            Button("Remove Device", role: .destructive) { if let d = removing { library.remove(d) }; removing = nil }
        } message: {
            Text("It stops syncing with this Mac. You can pair it again later.")
        }
    }

    private func pendingRow(_ p: PendingDevice) -> some View {
        HStack(spacing: 12) {
            RowIcon(symbol: p.symbol, tint: .orange)
            VStack(alignment: .leading, spacing: 2) {
                Text(p.id)
                Text("\(p.platform) · joined with a pairing code").font(.caption).foregroundStyle(.secondary)
                Label(p.fingerprint, systemImage: "checkmark.shield")
                    .font(.system(size: 12, weight: .medium, design: .monospaced)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Button("Reject") { library.reject(p) }
            Button("Approve") { library.approve(p) }.buttonStyle(.borderedProminent)
        }
    }

    private func deviceRow(_ d: Device) -> some View {
        HStack(spacing: 12) {
            RowIcon(symbol: d.symbol, tint: d.state == .offline ? .gray : .blue)
            VStack(alignment: .leading, spacing: 2) {
                Text(d.id)
                Text("\(d.platform) · " + (d.state == .offline ? "Last seen \(d.lastSeen)" : "Active now"))
                    .font(.caption).foregroundStyle(.secondary)
                if !d.fingerprint.isEmpty {
                    Text(d.fingerprint).font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary)
                }
            }
            Spacer(minLength: 8)
            HStack(spacing: 6) {
                Circle().fill(d.color).frame(width: 8, height: 8)
                Text(d.status).foregroundStyle(.secondary)
            }
            if d.state != .this {
                Menu {
                    Button("Remove Device…", systemImage: "trash", role: .destructive) { removing = d }
                } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .accessibilityLabel("More actions for \(d.id)")
            }
        }
    }
}

private struct AddDeviceSheet: View {
    @Environment(\.dismiss) private var dismiss
    enum Mode: String, CaseIterable { case enter = "Enter Code", show = "Show Code" }
    @State private var mode = Mode.enter
    @State private var code = ""
    @State private var waiting = false
    @State private var shown = String(format: "%03d %03d", Int.random(in: 0...999), Int.random(in: 0...999))
    @FocusState private var focused: Bool

    private var digits: String { code.filter(\.isNumber) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Add Device").font(.title3.weight(.semibold))
            Picker("Mode", selection: $mode) { ForEach(Mode.allCases, id: \.self) { Text($0.rawValue) } }
                .pickerStyle(.segmented).labelsHidden()
            Group { if mode == .enter { enter } else { show } }
                .frame(maxWidth: .infinity, minHeight: 130)
            Label("A new device always waits for approval, and both sides show the same fingerprint to compare.",
                  systemImage: "checkmark.shield")
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button(waiting ? "Done" : "Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                if mode == .enter && !waiting {
                    Button("Connect") { waiting = true }.keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent).disabled(digits.count != 6)
                }
            }
            .controlSize(.large)
        }
        .padding(22).frame(width: 420)
        .onAppear { focused = true }
    }

    @ViewBuilder private var enter: some View {
        if waiting {
            VStack(spacing: 10) {
                ProgressView()
                Text("Waiting for the other device to approve").font(.headline)
                Text("Fingerprint  C2D4 7A19 0E5B 86F3").font(.system(size: 13, design: .monospaced)).foregroundStyle(.secondary)
            }
        } else {
            VStack(spacing: 10) {
                TextField("000 000", text: $code)
                    .textFieldStyle(.plain).multilineTextAlignment(.center)
                    .font(.system(size: 34, weight: .semibold, design: .monospaced)).focused($focused)
                    .frame(height: 64)
                    .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .onChange(of: code) { _, v in code = String(v.filter(\.isNumber).prefix(6)) }
                Text("Enter the 6-digit code shown on your other device.").font(.callout).foregroundStyle(.secondary)
            }
        }
    }

    private var show: some View {
        VStack(spacing: 10) {
            Text(shown).font(.system(size: 40, weight: .semibold, design: .monospaced))
                .frame(maxWidth: .infinity).frame(height: 72)
                .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                .textSelection(.enabled)
            Text("Type this code on the other device. It works once and expires in 5 minutes.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
    }
}

// MARK: - Plugins

private struct PluginsPane: View {
    @EnvironmentObject var library: Library
    @State private var installing = false
    @State private var showingSettingsPreview = false
    @State private var settingsPreview: JSONValue?

    var body: some View {
        Form {
            if library.restartRequired {
                Section {
                    HStack {
                        Label("Restart Unic to apply plugin changes.", systemImage: "arrow.clockwise.circle.fill")
                            .foregroundStyle(.orange)
                        Spacer()
                        Button("Restart Now") { library.restartRequired = false }
                    }
                }
            }
            ForEach(Plugin.Stage.allCases, id: \.self) { stage in
                let items = library.plugins.filter { $0.stage == stage }
                if !items.isEmpty {
                    Section {
                        ForEach(Array(items.enumerated()), id: \.element.id) { i, plugin in
                            row(plugin, order: stage == .processing ? i + 1 : nil)
                        }
                    } header: {
                        Text(stage.rawValue)
                    } footer: {
                        Text(stage == .processing ? "Run in this order on every copied item, before it is saved or synced."
                                                  : "Moves items between your devices.")
                    }
                }
            }
            Section {
                Button("Preview plugin settings…", systemImage: "slider.horizontal.3") { showingSettingsPreview = true }
                Button("Install from GitHub…", systemImage: "plus") { installing = true }
            }
        }
        .formStyle(.grouped)
        .navigationTitle("Plugins")
        .sheet(isPresented: $installing) { InstallPluginSheet().environmentObject(library) }
        .sheet(isPresented: $showingSettingsPreview) { PluginSettingsPreview(saved: $settingsPreview) }
    }

    private func row(_ plugin: Plugin, order: Int?) -> some View {
        HStack(alignment: .top, spacing: 12) {
            RowIcon(symbol: plugin.symbol, tint: plugin.tint)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    if let order { Text("\(order).").foregroundStyle(.tertiary).monospacedDigit() }
                    Text(plugin.name)
                    Text(plugin.id).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(.secondary)
                }
                Text(plugin.summary).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Text("\(plugin.runtime.rawValue) · \(plugin.builtin ? "Built-in" : plugin.source) · v\(plugin.version)")
                    .font(.caption).foregroundStyle(.tertiary)
                if plugin.id == "builtin.tailcat" && !plugin.enabled {
                    Label("Sync is off. This Mac keeps history locally only.", systemImage: "info.circle")
                        .font(.callout).foregroundStyle(.orange)
                }
            }
            Spacer(minLength: 8)
            Toggle("Enabled", isOn: Binding(get: { plugin.enabled }, set: { _ in library.togglePlugin(plugin.id) }))
                .labelsHidden()
        }
        .contextMenu {
            if !plugin.builtin {
                Button("Uninstall", systemImage: "trash", role: .destructive) { library.uninstall(plugin.id) }
            }
        }
    }
}

private struct InstallPluginSheet: View {
    @EnvironmentObject var library: Library
    @Environment(\.dismiss) private var dismiss
    @State private var repo = ""
    @FocusState private var focused: Bool

    private var valid: Bool {
        let parts = repo.trimmingCharacters(in: .whitespaces).split(separator: "/", omittingEmptySubsequences: false)
        return parts.count == 2 && parts.allSatisfy { !$0.isEmpty && !$0.contains(" ") }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Install from GitHub").font(.title3.weight(.semibold))
            TextField("owner/repository", text: $repo).textFieldStyle(.roundedBorder).focused($focused).onSubmit(install)
            Label("Plugins run as separate programs on this Mac. Only install code you trust.", systemImage: "exclamationmark.shield")
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Install", action: install).keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent).disabled(!valid)
            }
            .controlSize(.large)
        }
        .padding(22).frame(width: 420)
        .onAppear { focused = true }
    }

    private func install() {
        guard valid else { return }
        library.install(repo: repo.trimmingCharacters(in: .whitespaces))
        dismiss()
    }
}
