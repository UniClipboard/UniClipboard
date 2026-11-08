import SwiftUI
import AppKit

/// Menu bar window: device status at a glance, recent items, entry points.
struct MenuPanel: View {
    @EnvironmentObject var library: Library
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Unic").font(.headline)
                Spacer()
                Label(library.paused ? "Paused" : "Syncing",
                      systemImage: library.paused ? "pause.circle.fill" : "checkmark.circle.fill")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(library.paused ? Color.orange : Color.green)
            }
            .padding(.horizontal, 14).padding(.top, 12)

            HStack(spacing: 0) {
                ForEach(library.devices) { d in
                    VStack(spacing: 5) {
                        Image(systemName: d.symbol).font(.system(size: 20)).frame(height: 26)
                            .overlay(alignment: .bottomTrailing) {
                                Circle().fill(d.color).frame(width: 8, height: 8)
                                    .overlay(Circle().stroke(.background, lineWidth: 2)).offset(x: 6, y: 2)
                            }
                            .foregroundStyle(d.state == .offline ? Color.secondary : Color.primary)
                        Text(d.id).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity)
                    .help(d.status)
                }
            }
            .padding(.horizontal, 6).padding(.vertical, 14)

            Divider()
            Text("Recent").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                .padding(.horizontal, 14).padding(.top, 10).padding(.bottom, 4)
            VStack(spacing: 0) {
                ForEach(library.live.prefix(5)) { clip in
                    Button { library.copy(clip.id) } label: {
                        HStack(spacing: 10) {
                            ClipThumb(clip: clip, size: 22)
                            Text(clip.title).lineLimit(1)
                            Spacer(minLength: 8)
                            Text(clip.time).font(.caption).foregroundStyle(.tertiary).monospacedDigit()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(HoverRowStyle())
                }
            }
            .padding(.horizontal, 6)

            Divider().padding(.vertical, 6)
            VStack(spacing: 0) {
                Button { QuickPanel.shared.show() } label: { MenuLabel(title: "Quick Paste", symbol: "magnifyingglass", key: "⌥⌘V") }
                Button {
                    openWindow(id: "library"); NSApp.activate(ignoringOtherApps: true)
                } label: { MenuLabel(title: "Open Unic", symbol: "square.grid.2x2") }
                Button { library.paused.toggle() } label: {
                    MenuLabel(title: library.paused ? "Resume Sync" : "Pause Sync", symbol: library.paused ? "play" : "pause")
                }
                SettingsLink { MenuLabel(title: "Settings…", symbol: "gearshape", key: "⌘,") }
                Button { NSApp.terminate(nil) } label: { MenuLabel(title: "Quit Unic", symbol: "power", key: "⌘Q") }
            }
            .buttonStyle(HoverRowStyle())
            .padding(.horizontal, 6).padding(.bottom, 6)
        }
        .frame(width: 330)
    }
}

private struct MenuLabel: View {
    let title: String
    let symbol: String
    var key: String?
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: symbol).frame(width: 20)
            Text(title)
            Spacer()
            if let key { Text(key).foregroundStyle(.tertiary) }
        }
        .contentShape(Rectangle())
    }
}

/// Menu-style row: transparent until hovered.
private struct HoverRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { Row(configuration: configuration) }
    private struct Row: View {
        let configuration: Configuration
        @State private var hovering = false
        var body: some View {
            configuration.label
                .padding(.horizontal, 8).padding(.vertical, 5)
                .background(hovering || configuration.isPressed ? Color.primary.opacity(configuration.isPressed ? 0.14 : 0.08) : .clear,
                            in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                .onHover { hovering = $0 }
        }
    }
}
