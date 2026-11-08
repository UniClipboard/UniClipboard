import SwiftUI
import AppKit
import Carbon.HIToolbox

/// Global ⌥⌘V. Carbon hot keys need no Accessibility permission (only pasting into another app does).
enum HotKey {
    private static var handler: (() -> Void)?
    private static var ref: EventHotKeyRef?

    static func register(_ action: @escaping () -> Void) {
        guard ref == nil else { return }
        handler = action
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in HotKey.handler?(); return noErr }, 1, &spec, nil, nil)
        let id = EventHotKeyID(signature: OSType(0x554E_4943), id: 1) // 'UNIC'
        RegisterEventHotKey(UInt32(kVK_ANSI_V), UInt32(cmdKey | optionKey), id, GetApplicationEventTarget(), 0, &ref)
    }
}

private final class KeyPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

/// Spotlight-style floating panel. Non-activating, so the app you were in stays frontmost.
@MainActor final class QuickPanel: NSObject, NSWindowDelegate {
    static let shared = QuickPanel()
    private(set) var window: NSPanel?
    static let size = NSSize(width: 780, height: 470)

    func toggle() { window?.isVisible == true ? close() : show() }

    func show() {
        let panel = window ?? make()
        window = panel
        Library.shared.panelSession = UUID()
        let mouse = NSEvent.mouseLocation
        if let frame = (NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main)?.visibleFrame {
            panel.setFrameOrigin(NSPoint(x: frame.midX - Self.size.width / 2,
                                         y: frame.minY + frame.height * 0.6 - Self.size.height / 2))
        }
        panel.makeKeyAndOrderFront(nil)
        panel.invalidateShadow()
    }

    func close() { window?.orderOut(nil) }

    nonisolated func windowDidResignKey(_ notification: Notification) {
        MainActor.assumeIsolated { close() }
    }

    private func make() -> NSPanel {
        let panel = KeyPanel(contentRect: NSRect(origin: .zero, size: Self.size),
                             styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isMovableByWindowBackground = true
        panel.delegate = self
        panel.contentView = NSHostingView(rootView: QuickPanelView().environmentObject(Library.shared))
        return panel
    }
}

struct QuickPanelView: View {
    @EnvironmentObject var library: Library
    @State private var query = ""
    @State private var index = 0
    @FocusState private var focused: Bool

    private var results: [Clip] { Array(library.search(query).prefix(40)) }
    private var current: Clip? { results.indices.contains(index) ? results[index] : nil }

    var body: some View {
        VStack(spacing: 0) {
            searchBar
            Divider()
            HStack(spacing: 0) {
                list.frame(width: 310)
                Divider()
                detail.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            Divider()
            footer
        }
        .frame(width: QuickPanel.size.width, height: QuickPanel.size.height)
        .modifier(PanelSurface())
        .background { shortcuts }
        .onAppear { focused = true }
        .onChange(of: library.panelSession) { query = ""; index = 0; focused = true }
        .onChange(of: query) { index = 0 }
    }

    private var searchBar: some View {
        HStack(spacing: 12) {
            Image(systemName: "magnifyingglass").font(.system(size: 20, weight: .medium)).foregroundStyle(.secondary)
            TextField("Search Clipboard", text: $query)
                .textFieldStyle(.plain).font(.system(size: 22))
                .focused($focused)
                .onSubmit(commit)
                .onKeyPress(.upArrow) { step(-1) }
                .onKeyPress(.downArrow) { step(1) }
                .onExitCommand { QuickPanel.shared.close() }
        }
        .padding(.horizontal, 20).frame(height: 62)
    }

    private var list: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(Array(results.enumerated()), id: \.element.id) { i, clip in
                        QuickRow(clip: clip, number: i < 9 ? i + 1 : nil, selected: i == index)
                            .id(clip.id)
                            .onTapGesture(count: 2) { index = i; commit() }
                            .simultaneousGesture(TapGesture().onEnded { index = i })
                    }
                }
                .padding(8)
            }
            .onChange(of: index) { _, i in
                if results.indices.contains(i) { proxy.scrollTo(results[i].id) }
            }
        }
    }

    @ViewBuilder private var detail: some View {
        if let clip = current {
            VStack(alignment: .leading, spacing: 0) {
                ScrollView {
                    ClipBody(clip: clip, large: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(20)
                }
                Divider().padding(.horizontal, 20)
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 5) {
                    GridRow { Text("From").foregroundStyle(.secondary); Text("\(clip.app) on \(clip.device)") }
                    GridRow { Text("Copied").foregroundStyle(.secondary); Text("\(clip.day), \(clip.time)") }
                    GridRow { Text("Size").foregroundStyle(.secondary); Text(clip.size) }
                }
                .font(.callout).padding(20)
            }
        } else {
            ContentUnavailableView.search(text: query)
        }
    }

    private var footer: some View {
        HStack(spacing: 18) {
            Label("Unic", systemImage: "doc.on.clipboard").font(.callout.weight(.medium)).foregroundStyle(.secondary)
            Spacer()
            KeyHint(title: "Copy", key: "↩")
            KeyHint(title: "Pin", key: "⌘P")
            KeyHint(title: "Pick", key: "⌘1–9")
            KeyHint(title: "Close", key: "esc")
        }
        .padding(.horizontal, 18).frame(height: 40)
    }

    /// Invisible buttons carry the ⌘-shortcuts so they work while the search field has focus.
    private var shortcuts: some View {
        ZStack {
            ForEach(1...9, id: \.self) { n in
                Button("") { if results.indices.contains(n - 1) { index = n - 1; commit() } }
                    .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: .command)
            }
            Button("") { if let clip = current { library.togglePin(clip.id) } }.keyboardShortcut("p")
        }
        .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
    }

    private func step(_ delta: Int) -> KeyPress.Result {
        guard !results.isEmpty else { return .ignored }
        index = min(max(index + delta, 0), results.count - 1)
        return .handled
    }

    private func commit() {
        guard let clip = current else { return }
        library.copy(clip.id)
        QuickPanel.shared.close()
    }
}

private struct QuickRow: View {
    let clip: Clip
    let number: Int?
    let selected: Bool
    var body: some View {
        HStack(spacing: 10) {
            ClipThumb(clip: clip, size: 28)
            VStack(alignment: .leading, spacing: 1) {
                Text(clip.title).lineLimit(1)
                    .font(clip.kind == .code ? .system(.body, design: .monospaced) : .body)
                Text("\(clip.app) · \(clip.device)").font(.caption).lineLimit(1)
                    .foregroundStyle(selected ? AnyShapeStyle(.white.opacity(0.8)) : AnyShapeStyle(.secondary))
            }
            Spacer(minLength: 4)
            if clip.pinned {
                Image(systemName: "pin.fill").font(.caption2)
                    .foregroundStyle(selected ? Color.white : Color.orange)
            }
            if let number {
                Text("⌘\(number)").font(.caption).monospacedDigit()
                    .foregroundStyle(selected ? AnyShapeStyle(.white.opacity(0.75)) : AnyShapeStyle(.tertiary))
            }
        }
        .foregroundStyle(selected ? Color.white : Color.primary)
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(selected ? Color.accentColor : Color.clear, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .contentShape(Rectangle())
    }
}

struct KeyHint: View {
    let title: String
    let key: String
    var body: some View {
        HStack(spacing: 6) {
            Text(title).foregroundStyle(.secondary)
            Text(key).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
        .font(.callout)
    }
}

/// Liquid Glass on macOS 26+, a behind-window material on earlier systems.
private struct PanelSurface: ViewModifier {
    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: 24, style: .continuous)
        if #available(macOS 26.0, *) {
            content.clipShape(shape).glassEffect(.regular, in: shape)
        } else {
            content
                .background(VisualEffect(material: .popover))
                .clipShape(shape)
                .overlay(shape.strokeBorder(Color.primary.opacity(0.1)))
        }
    }
}

struct VisualEffect: NSViewRepresentable {
    var material: NSVisualEffectView.Material
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }
    func updateNSView(_ view: NSVisualEffectView, context: Context) { view.material = material }
}
