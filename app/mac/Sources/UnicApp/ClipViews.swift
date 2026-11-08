import SwiftUI
import AppKit

/// Settings-style colored tile naming a content type.
struct KindBadge: View {
    let kind: Clip.Kind
    var size: CGFloat = 28
    var body: some View {
        Image(systemName: kind.symbol)
            .font(.system(size: size * 0.48, weight: .semibold)).foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(kind.tint.gradient, in: RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
    }
}

/// Thumbnail used for lists: the image itself for images, otherwise the type badge.
struct ClipThumb: View {
    let clip: Clip
    var size: CGFloat = 26
    var body: some View {
        if clip.kind == .image {
            SampleImage(variant: clip.art).frame(width: size, height: size)
                .clipShape(RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
        } else {
            KindBadge(kind: clip.kind, size: size)
        }
    }
}

// MARK: - Surfaces

/// Immersive, near-black canvas in dark mode; tiles and the detail panel step up from it.
enum Palette {
    static let canvas = dynamic(light: 0.955, dark: 0.065)
    static let tile = dynamic(light: 1.0, dark: 0.135)
    static let panel = dynamic(light: 1.0, dark: 0.105)
    private static func dynamic(light: CGFloat, dark: CGFloat) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            NSColor(white: appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light, alpha: 1)
        })
    }
}

/// The real icon of the app the item came from, so the source reads at a glance.
struct AppIcon: View {
    let app: String
    var size: CGFloat = 18
    private static let bundles = [
        "Terminal": "com.apple.Terminal", "Mail": "com.apple.mail", "Xcode": "com.apple.dt.Xcode",
        "Safari": "com.apple.Safari", "Screenshot": "com.apple.screenshot.launcher", "Notes": "com.apple.Notes",
        "VS Code": "com.microsoft.VSCode", "Photos": "com.apple.Photos", "Files": "com.apple.finder",
    ]
    private static var cache: [String: NSImage] = [:]
    private static func icon(for app: String) -> NSImage? {
        if let hit = cache[app] { return hit }
        guard let id = bundles[app], let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id) else { return nil }
        let image = NSWorkspace.shared.icon(forFile: url.path)
        cache[app] = image
        return image
    }
    var body: some View {
        if let image = Self.icon(for: app) {
            Image(nsImage: image).resizable().interpolation(.high).frame(width: size, height: size)
        } else {
            Image(systemName: "app.dashed").font(.system(size: size * 0.8)).frame(width: size, height: size)
        }
    }
}

// MARK: - Gallery tile

/// Content fills the whole tile; source and age sit on top of it. Images and links get a scrim.
struct ClipTile: View {
    let clip: Clip
    var selected = false
    var large = false
    @State private var hovering = false
    static let radius: CGFloat = 16

    private var onMedia: Bool { clip.kind == .image || clip.kind == .link }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: large ? 18 : Self.radius, style: .continuous)
        Color.clear
            .aspectRatio(4 / 3, contentMode: .fit)
            .overlay { TileArt(clip: clip, large: large) }
            .overlay {
                if onMedia && !large {
                    LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .center, endPoint: .bottom)
                }
            }
            .overlay(alignment: .bottomLeading) { if !large { caption } }
            .clipShape(shape)
            .overlay {
                shape.strokeBorder(selected ? Color.primary.opacity(0.85) : Color.primary.opacity(0.07),
                                   lineWidth: selected ? 2.5 : 1)
            }
            .brightness(hovering && !large ? 0.03 : 0)
            .animation(.snappy(duration: 0.15), value: hovering)
            .onHover { hovering = $0 }
            .contentShape(shape)
            .accessibilityElement(children: .combine)
            .accessibilityLabel("\(clip.kind.singular), \(clip.title), from \(clip.app), \(clip.ago)")
            .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var caption: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let headline {
                Text(headline).font(.system(size: 15, weight: .semibold)).lineLimit(1)
            }
            HStack(spacing: 7) {
                AppIcon(app: clip.app, size: 18)
                if clip.pinned { Image(systemName: "pin.fill").font(.caption2) }
                Text(clip.ago).lineLimit(1)
                Spacer(minLength: 6)
                if clip.kind == .image || clip.kind == .file { Text(clip.size).lineLimit(1) }
            }
            .font(.system(size: 13)).monospacedDigit()
            .foregroundStyle(onMedia ? AnyShapeStyle(.white.opacity(0.8)) : AnyShapeStyle(.secondary))
        }
        .foregroundStyle(onMedia ? Color.white : Color.primary)
        .padding(.horizontal, 14).padding(.bottom, 12)
    }

    private var headline: String? {
        switch clip.kind {
        case .image, .file: return clip.name
        case .link: return clip.host
        default: return nil
        }
    }
}

/// What fills a tile. `large` is the hero preview in the detail panel.
struct TileArt: View {
    let clip: Clip
    var large = false
    private var s: CGFloat { large ? 1.15 : 1 }

    var body: some View {
        switch clip.kind {
        case .image:
            SampleImage(variant: clip.art)
        case .link:
            let tint = Favicon.tint(for: clip.host)
            ZStack(alignment: .topLeading) {
                Color.black
                LinearGradient(colors: [tint, tint.opacity(0.4)], startPoint: .topLeading, endPoint: .bottomTrailing)
                VStack(alignment: .leading, spacing: 8) {
                    Favicon(host: clip.host, size: 30 * s)
                    if large {
                        Spacer()
                        Text(clip.host).font(.system(size: 24, weight: .bold))
                        Text(clip.path).font(.callout).opacity(0.75).lineLimit(1)
                    }
                }
                .foregroundStyle(.white)
                .padding(16 * s)
            }
        case .file:
            ZStack(alignment: .top) {
                Palette.tile
                PageView(text: clip.body, mini: !large)
                    .frame(width: large ? 170 : 92)
                    .rotationEffect(.degrees(-3))
                    .padding(.top, large ? 22 : 16)
            }
        case .code:
            surface {
                Text(clip.body)
                    .font(.system(size: 12.5 * s, design: .monospaced)).lineSpacing(3)
                    .modifier(SelectableText(enabled: large))
            }
        case .text:
            surface {
                if !clip.body.contains("\n") && clip.body.count <= 32 {
                    Text(clip.body).font(.system(size: 20 * s, weight: .semibold)).lineLimit(2)
                } else {
                    Text(clip.body).font(.system(size: 15 * s, weight: .medium)).lineSpacing(4)
                        .lineLimit(large ? nil : 3)
                }
            }
        }
    }

    private func surface<C: View>(@ViewBuilder _ content: () -> C) -> some View {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(.horizontal, 16 * s).padding(.top, 18 * s).padding(.bottom, large ? 18 : 46)
            .background(Palette.tile)
    }
}

struct SelectableText: ViewModifier {
    let enabled: Bool
    func body(content: Content) -> some View {
        if enabled { content.textSelection(.enabled) } else { content }
    }
}

// MARK: - Content renderers

/// Renders what was copied. `large` is used for previews, where text becomes selectable.
struct ClipBody: View {
    let clip: Clip
    var large = false

    var body: some View {
        switch clip.kind {
        case .code: code
        case .link: link
        case .image: image
        case .file: file
        case .text: text
        }
    }

    private var code: some View {
        Text(clip.body)
            .font(.system(size: large ? 14 : 11.5, design: .monospaced))
            .lineSpacing(large ? 4 : 2)
            .modifier(Selectable(enabled: large))
            .padding(large ? 18 : 10)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: large ? 12 : 9, style: .continuous))
            .padding(.horizontal, large ? 0 : 8)
    }

    private var link: some View {
        VStack(alignment: .leading, spacing: large ? 10 : 6) {
            Favicon(host: clip.host, size: large ? 48 : 32)
            Text(clip.host).font(large ? .title.weight(.semibold) : .title3.weight(.semibold)).lineLimit(1)
            Text(clip.path.isEmpty ? clip.body : clip.path)
                .font(large ? .body : .callout).foregroundStyle(.secondary).lineLimit(2)
            if large, let url = URL(string: clip.body) {
                Link(destination: url) { Label("Open in Browser", systemImage: "safari") }.padding(.top, 4)
            }
        }
        .padding(.horizontal, large ? 0 : 12).padding(.top, large ? 0 : 6)
    }

    @ViewBuilder private var image: some View {
        let shape = RoundedRectangle(cornerRadius: large ? 12 : 9, style: .continuous)
        if large {
            SampleImage(variant: clip.art).aspectRatio(16 / 10, contentMode: .fit).clipShape(shape)
        } else {
            SampleImage(variant: clip.art).clipShape(shape).padding(.horizontal, 8)
        }
    }

    @ViewBuilder private var file: some View {
        if large {
            PageView(text: clip.body).frame(width: 300).frame(maxWidth: .infinity)
        } else {
            HStack(alignment: .top, spacing: 14) {
                PageView(text: clip.body, mini: true).frame(width: 66)
                VStack(alignment: .leading, spacing: 4) {
                    Text(clip.name).font(.headline).lineLimit(2)
                    Text("PDF Document").font(.caption).foregroundStyle(.secondary)
                    Text(clip.size).font(.caption).foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 12).padding(.top, 4)
        }
    }

    @ViewBuilder private var text: some View {
        let lines = clip.body.split(separator: "\n", maxSplits: 1).map(String.init)
        if lines.count == 1 && clip.body.count <= 48 {
            Text(clip.body).font(large ? .largeTitle.weight(.medium) : .title2.weight(.medium))
                .lineLimit(3).modifier(Selectable(enabled: large))
                .padding(.horizontal, large ? 0 : 12).padding(.top, large ? 0 : 6)
        } else {
            VStack(alignment: .leading, spacing: large ? 8 : 4) {
                Text(lines[0]).font(large ? .title2.weight(.semibold) : .headline).lineLimit(2)
                if lines.count > 1 {
                    Text(lines[1]).font(large ? .title3 : .callout).foregroundStyle(.secondary)
                        .lineSpacing(large ? 5 : 2)
                }
            }
            .modifier(Selectable(enabled: large))
            .padding(.horizontal, large ? 0 : 12).padding(.top, large ? 0 : 4)
        }
    }
}

private struct Selectable: ViewModifier {
    let enabled: Bool
    func body(content: Content) -> some View {
        if enabled { content.textSelection(.enabled) } else { content }
    }
}

struct Favicon: View {
    let host: String
    var size: CGFloat = 32
    private var color: Color {
        if host.contains("github") { return Color(white: 0.15) }
        if host.contains("apple") { return Color(white: 0.55) }
        let palette: [Color] = [.blue, .indigo, .teal, .pink, .green]
        return palette[host.unicodeScalars.reduce(0) { $0 + Int($1.value) } % palette.count]
    }
    /// Hero color for a site's link tile.
    static func tint(for host: String) -> Color {
        if host.contains("github") { return Color(red: 0.29, green: 0.25, blue: 0.62) }
        if host.contains("apple") { return Color(red: 0.12, green: 0.45, blue: 0.85) }
        let palette: [Color] = [.blue, .indigo, .teal, .pink, .green]
        return palette[host.unicodeScalars.reduce(0) { $0 + Int($1.value) } % palette.count]
    }
    var body: some View {
        Text(host.first.map { String($0).uppercased() } ?? "?")
            .font(.system(size: size * 0.5, weight: .bold, design: .rounded)).foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color.gradient, in: RoundedRectangle(cornerRadius: size * 0.24, style: .continuous))
    }
}

/// A sheet of paper showing the document's first page.
struct PageView: View {
    let text: String
    var mini = false
    var body: some View {
        Text(text)
            .font(.system(size: mini ? 3.6 : 9, design: .monospaced))
            .foregroundStyle(Color(white: 0.2))
            .lineSpacing(mini ? 0.5 : 3)
            .padding(mini ? 7 : 22)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .aspectRatio(8.5 / 11, contentMode: .fit)
            .background(.white, in: RoundedRectangle(cornerRadius: mini ? 3 : 6, style: .continuous))
            .overlay(alignment: .bottomTrailing) {
                Text("PDF").font(.system(size: mini ? 7 : 11, weight: .heavy)).foregroundStyle(.white)
                    .padding(.horizontal, mini ? 4 : 6).padding(.vertical, mini ? 1 : 2)
                    .background(.red, in: RoundedRectangle(cornerRadius: 3)).padding(mini ? 5 : 12)
            }
            .shadow(color: .black.opacity(0.18), radius: mini ? 2 : 8, y: mini ? 1 : 3)
    }
}

/// Placeholder artwork standing in for real image content.
struct SampleImage: View {
    var variant = 0
    var body: some View {
        let colors: [Color] = variant == 0 ? [.indigo, .purple, .pink.opacity(0.8)] : [.orange, .pink, .yellow.opacity(0.9)]
        GeometryReader { geo in
            LinearGradient(colors: colors, startPoint: .topLeading, endPoint: .bottomTrailing)
                .overlay {
                    Image(systemName: variant == 0 ? "mountain.2.fill" : "sun.horizon.fill")
                        .font(.system(size: min(geo.size.width, geo.size.height) * 0.42))
                        .foregroundStyle(.white.opacity(0.88))
                }
        }
    }
}
