import SwiftUI

/// Main window, Finder-style: the sidebar chooses what to show, the main column shows it.
/// Device and plugin management live in Settings, reached from the sidebar footer.
struct LibraryWindow: View {
    @EnvironmentObject var library: Library
    @State private var tagging: String?

    var body: some View {
        NavigationSplitView {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 220, max: 280)
        } detail: {
            ClipboardPage(tagging: $tagging)
                .background(Palette.canvas.ignoresSafeArea())
        }
        .overlay(alignment: .bottom) { Toast() }
        .animation(.snappy, value: library.notice)
        .sheet(item: Binding(get: { library.clip(library.previewing) }, set: { library.previewing = $0?.id })) { clip in
            PreviewSheet(clip: clip).environmentObject(library)
        }
        .sheet(isPresented: Binding(get: { tagging != nil }, set: { if !$0 { tagging = nil } })) {
            TagSheet(clipID: tagging).environmentObject(library)
        }
    }
}

// MARK: - Sidebar

/// Every row is a filter. Counts are quiet badges; devices show whether they are reachable.
private struct Sidebar: View {
    @EnvironmentObject var library: Library

    var body: some View {
        List(selection: Binding(get: { library.filter }, set: { if let f = $0 { library.filter = f } })) {
            row(.all)
            row(.pinned)

            Section("Types") {
                ForEach(Clip.Kind.allCases, id: \.self) { row(.kind($0)) }
            }

            Section("Devices") {
                ForEach(library.devices) { d in
                    Label {
                        HStack(spacing: 6) {
                            Text(d.id)
                            Spacer(minLength: 4)
                            Circle().fill(d.color).frame(width: 7, height: 7).help(d.status)
                        }
                    } icon: { Image(systemName: d.symbol) }
                    .tag(Filter.device(d.id))
                }
            }

            Section {
                ForEach(library.customTags, id: \.self) { tag in
                    row(.tag(tag))
                        .contextMenu {
                            Button("Delete Tag “\(tag)”", systemImage: "trash", role: .destructive) { library.removeTag(tag) }
                        }
                }
            } header: {
                HStack {
                    Text("Tags")
                    Spacer()
                    AddTagButton()
                }
            }

            Section { row(.deleted) }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) { footer }
    }

    private func row(_ f: Filter) -> some View {
        Label(f.title, systemImage: f.symbol).badge(library.count(f)).tag(f)
    }

    private var footer: some View {
        SettingsLink {
            Label("Settings", systemImage: "gearshape")
            Spacer(minLength: 4)
            if !library.pending.isEmpty {
                Text("\(library.pending.count)").font(.caption.weight(.semibold)).foregroundStyle(.white)
                    .padding(.horizontal, 6).frame(minWidth: 18, minHeight: 18)
                    .background(Color.orange, in: Capsule())
                    .help("Devices waiting for your approval")
            } else {
                Text("⌘,").foregroundStyle(.tertiary)
            }
        }
        .buttonStyle(SidebarFooterStyle())
        .padding(.horizontal, 10).padding(.bottom, 10)
    }
}

/// "+" in the Tags header: names a new tag, which becomes a filter of its own.
private struct AddTagButton: View {
    @EnvironmentObject var library: Library
    @State private var open = false
    @State private var name = ""
    @FocusState private var focused: Bool

    var body: some View {
        Button { name = ""; open = true } label: {
            Image(systemName: "plus").font(.system(size: 11, weight: .bold)).frame(width: 20, height: 20).contentShape(Rectangle())
        }
        .buttonStyle(.plain).foregroundStyle(.secondary)
        .help("New Tag")
        .accessibilityLabel("New Tag")
        .popover(isPresented: $open, arrowEdge: .trailing) {
            VStack(alignment: .leading, spacing: 10) {
                Text("New Tag").font(.headline)
                TextField("Tag name", text: $name).textFieldStyle(.roundedBorder).focused($focused).onSubmit(create)
                    .frame(width: 220)
                HStack {
                    Spacer()
                    Button("Cancel") { open = false }.keyboardShortcut(.cancelAction)
                    Button("Add", action: create).keyboardShortcut(.defaultAction)
                        .disabled(Library.clean(name).isEmpty)
                }
            }
            .padding(16)
            .onAppear { focused = true }
        }
    }

    private func create() {
        guard let tag = library.addTag(name) else { return }
        library.filter = .tag(tag)
        open = false
    }
}

/// Footer row that reads like a sidebar item: quiet until hovered.
private struct SidebarFooterStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { Row(configuration: configuration) }
    private struct Row: View {
        let configuration: Configuration
        @State private var hovering = false
        var body: some View {
            HStack { configuration.label }
                .font(.system(size: 13))
                .foregroundStyle(hovering ? Color.primary : Color.secondary)
                .padding(.horizontal, 10).frame(height: 32)
                .background(Color.primary.opacity(configuration.isPressed ? 0.12 : hovering ? 0.07 : 0),
                            in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                .contentShape(Rectangle())
                .onHover { hovering = $0 }
        }
    }
}

private struct Toast: View {
    @EnvironmentObject var library: Library
    var body: some View {
        if let notice = library.notice {
            Label(notice, systemImage: "checkmark.circle.fill")
                .font(.callout.weight(.medium))
                .padding(.horizontal, 16).padding(.vertical, 10)
                .modifier(FloatingSurface())
                .padding(.bottom, 24)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                .allowsHitTesting(false)
        }
    }
}

/// Sync health; click to pause or resume.
private struct SyncStatus: View {
    @EnvironmentObject var library: Library
    var body: some View {
        Button { library.paused.toggle() } label: {
            HStack(spacing: 6) {
                Circle().fill(library.paused ? Color.orange : Color.green).frame(width: 7, height: 7)
                Text(library.paused ? "Sync Paused" : "Synced · \(library.onlineCount) devices")
            }
            .font(.system(size: 13)).foregroundStyle(.secondary).lineLimit(1)
            .padding(.horizontal, 6)
        }
        .buttonStyle(.plain)
        .help(library.paused ? "Resume Sync" : "Pause Sync")
    }
}

// MARK: - Clipboard page

/// The gallery for the selected filter. The detail panel floats over it and never reflows the grid.
private struct ClipboardPage: View {
    @EnvironmentObject var library: Library
    @Binding var tagging: String?

    var body: some View {
        Gallery(tagging: $tagging)
            .overlay(alignment: .trailing) {
                if library.showInspector && library.current != nil {
                    DetailPanel(tagging: $tagging)
                        .frame(width: 350)
                        .padding(.trailing, 12).padding(.bottom, 12).padding(.top, 2)
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            .animation(.snappy(duration: 0.25), value: library.showInspector)
            .navigationTitle(library.filter.title)
            .navigationSubtitle(library.items.count == 1 ? "1 item" : "\(library.items.count) items")
            .searchable(text: $library.query, placement: .toolbar, prompt: "Search")
            .toolbar { ToolbarItem(placement: .automatic) { SyncStatus() } }
    }
}

// MARK: - Gallery

private struct Gallery: View {
    @EnvironmentObject var library: Library
    @Binding var tagging: String?
    @State private var columns = 3
    @FocusState private var focused: Bool

    static let margin: CGFloat = 20
    static let gap: CGFloat = 14
    static let tileMin: CGFloat = 180

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
                        GeometryReader { geo in
                ScrollViewReader { proxy in
                    ScrollView {
                        VStack(alignment: .leading, spacing: 14) {
                            if library.showDeleted {
                                Text("Items are removed permanently after 30 days. Press Delete to restore.")
                                    .font(.callout).foregroundStyle(.secondary)
                            }
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: Self.tileMin), spacing: Self.gap)], spacing: Self.gap) {
                                ForEach(library.sections.flatMap(\.clips)) { tile($0).id($0.id) }
                            }
                        }
                        .padding(.horizontal, Self.margin).padding(.vertical, Self.margin)
                    }
                    .onChange(of: library.selection) { _, id in
                        if let id { withAnimation(.snappy) { proxy.scrollTo(id) } }
                    }
                }
                .onAppear { columns = Self.columns(for: geo.size.width); focused = true }
                .onChange(of: geo.size.width) { _, w in columns = Self.columns(for: w) }
            }
            .overlay { emptyState }
        }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .onKeyPress(.upArrow) { move(-columns) }
        .onKeyPress(.downArrow) { move(columns) }
        .onKeyPress(.space) { library.previewing = library.selection; return .handled }
        .onExitCommand { library.showInspector = false }
        .onKeyPress(.return) { library.copy(); return .handled }
        .onKeyPress(.delete) {
            if library.showDeleted { library.restore() } else { library.delete() }
            return .handled
        }
        .onChange(of: library.filter) { keepSelectionVisible() }
        .onChange(of: library.query) { keepSelectionVisible() }
    }

    private func tile(_ clip: Clip) -> some View {
        ClipTile(clip: clip, selected: library.selection == clip.id)
            .onTapGesture(count: 2) { library.selection = clip.id; library.copy(clip.id) }
            .simultaneousGesture(TapGesture().onEnded {
                library.selection = clip.id; library.showInspector = true; focused = true
            })
            .contextMenu { ClipMenu(clip: clip, tagging: $tagging) }
    }

    private static func columns(for width: CGFloat) -> Int {
        max(1, Int((width - margin * 2 + gap) / (tileMin + gap)))
    }

    private func move(_ delta: Int) -> KeyPress.Result {
        let ids = library.orderedIDs
        guard !ids.isEmpty else { return .ignored }
        guard let i = ids.firstIndex(where: { $0 == library.selection }) else {
            library.selection = ids[0]; return .handled
        }
        library.selection = ids[min(max(i + delta, 0), ids.count - 1)]
        return .handled
    }

    private func keepSelectionVisible() {
        if library.current == nil { library.selection = library.orderedIDs.first }
    }

    @ViewBuilder private var emptyState: some View {
        if library.items.isEmpty {
            if !library.query.isEmpty {
                ContentUnavailableView.search(text: library.query)
            } else if library.showDeleted {
                ContentUnavailableView("No Recently Deleted Items", systemImage: "trash")
            } else {
                ContentUnavailableView("Nothing Here Yet", systemImage: "doc.on.clipboard",
                                       description: Text("Copy something on any of your devices."))
            }
        }
    }
}

// MARK: - Detail panel

/// Floating panel: actions in its header, the content as a hero, labeled facts, one primary action.
private struct DetailPanel: View {
    @EnvironmentObject var library: Library
    @Binding var tagging: String?

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 22, style: .continuous)
        VStack(spacing: 0) {
            if let clip = library.current {
                header(clip)
                Divider().opacity(0.6)
                ScrollView { facts(clip).padding(18) }.defaultScrollAnchor(.top)
                footer(clip)
            } else {
                ContentUnavailableView("No Selection", systemImage: "square.dashed",
                                       description: Text("Select an item to see its details."))
            }
        }
        .background(Palette.panel, in: shape)
        .overlay(shape.strokeBorder(Color.primary.opacity(0.08)))
        .shadow(color: .black.opacity(0.12), radius: 18, y: 6)
    }

    private func header(_ clip: Clip) -> some View {
        HStack(spacing: 10) {
            Image(systemName: clip.kind.outline).font(.system(size: 13, weight: .semibold))
                .frame(width: 32, height: 32).background(Color.primary.opacity(0.08), in: Circle())
            Text(clip.kind.singular).font(.system(size: 15, weight: .semibold))
            Spacer()
            if clip.deleted {
                CircleButton(symbol: "arrow.uturn.backward", help: "Restore") { library.restore(clip.id) }
            } else {
                CircleButton(symbol: clip.pinned ? "pin.fill" : "pin", help: clip.pinned ? "Unpin" : "Pin",
                             tint: clip.pinned ? .accentColor : .primary) { library.togglePin(clip.id) }
                CircleButton(symbol: "trash", help: "Move to Recently Deleted") { library.delete(clip.id) }
            }
            CircleButton(symbol: "xmark", help: "Close") { library.showInspector = false }
        }
        .padding(.horizontal, 14).frame(height: 58)
    }

    private func facts(_ clip: Clip) -> some View {
        VStack(alignment: .leading, spacing: 20) {
            ClipTile(clip: clip, large: true)
                .onTapGesture(count: 2) { library.previewing = clip.id }
                .help("Double-click or press Space for Quick Look")

            fact("Created") { Text("\(clip.day) at \(clip.time)") }

            fact("Source") {
                HStack(spacing: 10) {
                    AppIcon(app: clip.app, size: 26)
                    Text(clip.app)
                    Text("on \(clip.device)").foregroundStyle(.secondary)
                }
            }

            if clip.kind == .link, let url = URL(string: clip.body) {
                fact("Link") {
                    HStack {
                        Text(clip.body).lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                        Spacer(minLength: 8)
                        Link(destination: url) {
                            Image(systemName: "arrow.up.right").font(.system(size: 12, weight: .bold))
                                .frame(width: 28, height: 28).background(Color.primary.opacity(0.1), in: Circle())
                        }
                        .buttonStyle(.plain).help("Open in Browser")
                    }
                    .padding(.leading, 14).padding(.trailing, 8).frame(height: 46)
                    .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
            }

            fact("Delivery") {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(library.devices) { d in
                        HStack(spacing: 10) {
                            Image(systemName: d.symbol).frame(width: 20).foregroundStyle(.secondary)
                            Text(d.id)
                            Spacer()
                            delivery(d, clip)
                        }
                    }
                }
            }

            fact("Tags") {
                HStack(spacing: 6) {
                    ForEach(clip.tags, id: \.self) { tag in
                        Text(tag).font(.system(size: 13, weight: .medium))
                            .padding(.horizontal, 10).frame(height: 26)
                            .background(Color.primary.opacity(0.08), in: Capsule())
                    }
                    if !clip.deleted {
                        Button { tagging = clip.id } label: {
                            Image(systemName: "plus").font(.system(size: 11, weight: .bold))
                                .frame(width: 26, height: 26).background(Color.primary.opacity(0.08), in: Circle())
                        }
                        .buttonStyle(.plain).help("Add Tag")
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func fact<C: View>(_ label: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(label.uppercased()).font(.system(size: 11, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
            content().font(.system(size: 14, weight: .medium))
        }
    }

    @ViewBuilder private func delivery(_ d: Device, _ clip: Clip) -> some View {
        let text = d.delivery(of: clip)
        if d.id == clip.device {
            Text(text).foregroundStyle(.secondary)
        } else if d.state == .offline {
            Label(text, systemImage: "clock").foregroundStyle(.orange)
        } else {
            Label(text, systemImage: "checkmark.circle.fill").foregroundStyle(.green)
        }
    }

    @ViewBuilder private func footer(_ clip: Clip) -> some View {
        if !clip.deleted {
            HStack(spacing: 8) {
                Button { library.copy(clip.id) } label: {
                    Label("Copy to Clipboard", systemImage: "doc.on.doc")
                }
                .buttonStyle(FilledButtonStyle())
                .keyboardShortcut("c", modifiers: [.command, .shift])
                Menu {
                    ForEach(library.targets) { d in
                        Button(d.id, systemImage: d.symbol) { library.send(clip.id, to: d.id) }
                    }
                } label: {
                    Image(systemName: "paperplane").font(.system(size: 15, weight: .semibold))
                        .frame(width: 46, height: 46)
                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .contentShape(Rectangle())
                }
                .menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
                .help("Send To")
            }
            .padding(12)
        }
    }
}

// MARK: - Controls

/// Full-width, high-contrast primary action (white in dark mode, black in light).
private struct FilledButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(Palette.canvas)
            .frame(maxWidth: .infinity).frame(height: 46)
            .background(Color.primary.opacity(configuration.isPressed ? 0.7 : 1),
                        in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .contentShape(Rectangle())
    }
}

/// Round icon button; interactive Liquid Glass on macOS 26+.
private struct CircleButton: View {
    let symbol: String
    let help: String
    var tint: Color = .primary
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 13, weight: .semibold)).foregroundStyle(tint)
                .frame(width: 32, height: 32).contentShape(Circle())
        }
        .buttonStyle(.plain)
        .modifier(CircleSurface())
        .help(help)
        .accessibilityLabel(help)
    }
}

private struct CircleSurface: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 26.0, *) {
            content.glassEffect(.regular.interactive(), in: .circle)
        } else {
            content.background(Color.primary.opacity(0.08), in: Circle())
        }
    }
}

// MARK: - Menus and sheets

struct ClipMenu: View {
    @EnvironmentObject var library: Library
    let clip: Clip
    @Binding var tagging: String?
    var body: some View {
        if clip.deleted {
            Button("Restore", systemImage: "arrow.uturn.backward") { library.restore(clip.id) }
        } else {
            Button("Copy", systemImage: "doc.on.doc") { library.copy(clip.id) }
            Button("Quick Look", systemImage: "eye") { library.previewing = clip.id }
            Menu("Send To") {
                ForEach(library.targets) { d in
                    Button(d.id, systemImage: d.symbol) { library.send(clip.id, to: d.id) }
                }
            }
            Divider()
            Button(clip.pinned ? "Unpin" : "Pin", systemImage: clip.pinned ? "pin.slash" : "pin") { library.togglePin(clip.id) }
            Menu("Tags", systemImage: "tag") {
                ForEach(library.customTags, id: \.self) { tag in
                    Toggle(tag, isOn: Binding(get: { clip.tags.contains(tag) }, set: { _ in library.toggleTag(tag, on: clip.id) }))
                }
                if !library.customTags.isEmpty { Divider() }
                Button("New Tag…", systemImage: "plus") { tagging = clip.id }
            }
            Divider()
            Button("Delete", systemImage: "trash", role: .destructive) { library.delete(clip.id) }
        }
    }
}

/// Quick Look: the content at reading size, nothing else.
private struct PreviewSheet: View {
    @EnvironmentObject var library: Library
    @Environment(\.dismiss) private var dismiss
    let clip: Clip

    var body: some View {
        VStack(spacing: 0) {
            ViewThatFits(in: .vertical) {
                content
                ScrollView { content }
            }
            .frame(maxHeight: 520)
            HStack {
                Text("\(clip.app) on \(clip.device) · \(clip.day), \(clip.time)")
                    .font(.callout).foregroundStyle(.secondary)
                Spacer()
                Button("Close") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Copy") { library.copy(clip.id); dismiss() }
                    .keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent)
            }
            .controlSize(.large)
            .padding(16)
        }
        .frame(width: 680)
        .onKeyPress(.space) { dismiss(); return .handled }
    }

    private var content: some View {
        ClipBody(clip: clip, large: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(28)
    }
}

private struct TagSheet: View {
    @EnvironmentObject var library: Library
    @Environment(\.dismiss) private var dismiss
    let clipID: String?
    @State private var tag = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add Tag").font(.headline)
            TextField("Tag name", text: $tag).textFieldStyle(.roundedBorder).onSubmit(add)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Add", action: add).keyboardShortcut(.defaultAction)
                    .disabled(tag.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20).frame(width: 320)
    }
    private func add() {
        guard let clean = library.addTag(tag) else { return }
        library.mutate(clipID) { if !$0.tags.contains(clean) { $0.tags.append(clean) } }
        dismiss()
    }
}

/// Glass is reserved for floating controls; content stays opaque.
struct FloatingSurface: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 26.0, *) {
            content.glassEffect(.regular, in: .capsule)
        } else {
            content.background(.regularMaterial, in: Capsule()).shadow(color: .black.opacity(0.12), radius: 8, y: 2)
        }
    }
}
