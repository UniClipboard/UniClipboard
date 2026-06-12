import SwiftUI
import UIKit

/// Fixed-size card for a two-column clipboard grid (Paste-app style).
/// Width is determined by the enclosing grid column; height matches the width
/// so every card renders as a 1:1 square.
///
/// Image cards use an immersive layout: the image fills the card height,
/// letterboxed horizontally with a checkerboard grid in the margins.
/// URL cards use an immersive layout: OG image fills the top 3/5,
/// title + URL fill the bottom 2/5.
/// Header/footer float over the image with gradient scrims.
struct ClipboardCard: View {
    let item: ClipboardHistoryItem
    let isLatest: Bool
    var thumbnailImage: UIImage? = nil
    var urlMetadata: URLCardMetadata? = nil
    var isLoading: Bool = false

    var body: some View {
        Group {
            switch item.entry.displayKind {
            case .image:
                imageCardBody
            case .url:
                urlCardBody
            case .text, .file, .group:
                standardCardBody
            }
        }
        .frame(maxWidth: .infinity)
        // 1:1 square — height tracks the grid column width instead of a fixed
        // point value, so two-column cards stay square on any device/orientation.
        .aspectRatio(1, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        // Opaque elevated surface — content cards must not use Liquid Glass.
        // secondarySystemGroupedBackground is the system color designed to sit
        // on systemGroupedBackground; a soft low-opacity shadow supplies the
        // Paste-style elevation cue without sampling a backdrop.
        .background(
            Color(.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
        .shadow(color: .black.opacity(0.06), radius: 5, y: 2)
        .overlay {
            if isLoading {
                Color(.systemBackground).opacity(0.6)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay {
                        ProgressView()
                    }
            }
        }
        .accessibilityElement(children: .combine)
    }

    // MARK: - Standard (text / file / group)

    private var standardCardBody: some View {
        VStack(alignment: .leading, spacing: 6) {
            headerRow(style: .normal)
            standardBodyContent
            Spacer(minLength: 0)
            bottomRow(style: .normal)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var standardBodyContent: some View {
        switch item.entry.displayKind {
        case .text:
            Text(item.entry.text)
                .font(.callout)
                .foregroundStyle(.primary)
                .lineLimit(3)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
        case .file, .group:
            fileBody
        case .image, .url:
            EmptyView()
        }
    }

    private var fileBody: some View {
        VStack(spacing: 6) {
            Image(systemName: item.entry.displayKind.symbolName)
                .font(.largeTitle)
                .foregroundStyle(item.entry.displayKind.tint)
            Text(item.entry.dataName ?? item.entry.text)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Immersive image card

    private var imageCardBody: some View {
        ZStack {
            CheckerboardBackground()
            if let thumbnail = thumbnailImage {
                Image(uiImage: thumbnail)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                imagePlaceholder
            }
            // Gradient scrims for header/footer legibility.
            VStack(spacing: 0) {
                LinearGradient(
                    colors: [.black.opacity(0.45), .clear],
                    startPoint: .top, endPoint: .bottom
                )
                .frame(height: 36)
                Spacer(minLength: 0)
                LinearGradient(
                    colors: [.clear, .black.opacity(0.35)],
                    startPoint: .top, endPoint: .bottom
                )
                .frame(height: 28)
            }
            // Overlaid header + footer
            VStack {
                headerRow(style: .overlay)
                Spacer(minLength: 0)
                bottomRow(style: .overlay)
            }
            .padding(10)
        }
    }

    private var imagePlaceholder: some View {
        Color(item.entry.displayKind.tint).opacity(0.12)
            .overlay {
                Image(systemName: "photo.fill")
                    .font(.largeTitle)
                    .foregroundStyle(item.entry.displayKind.tint.opacity(0.5))
            }
    }

    // MARK: - Immersive URL card

    private var urlCardBody: some View {
        // OG image fills the top 3/5 of the live card height; title + URL fill
        // the bottom 2/5. Read the height from geometry since the card is now
        // sized by aspect ratio rather than a fixed point height.
        GeometryReader { proxy in
            VStack(spacing: 0) {
                ZStack {
                    if let ogImage = urlMetadata?.ogImage {
                        Color.clear.overlay {
                            Image(uiImage: ogImage)
                                .resizable()
                                .scaledToFill()
                        }
                        .clipped()
                    } else {
                        urlPlaceholder
                    }
                    VStack(spacing: 0) {
                        LinearGradient(
                            colors: [.black.opacity(0.45), .clear],
                            startPoint: .top, endPoint: .bottom
                        )
                        .frame(height: 36)
                        Spacer(minLength: 0)
                    }
                    VStack {
                        headerRow(style: .overlay)
                        Spacer(minLength: 0)
                    }
                    .padding(10)
                }
                .frame(height: proxy.size.height * 3 / 5)
                .clipped()

                VStack(alignment: .leading, spacing: 2) {
                    Text(urlMetadata?.title ?? urlDomain)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text(item.entry.urlWithoutScheme)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer(minLength: 0)
                    bottomRow(style: .normal)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            }
        }
    }

    private var urlPlaceholder: some View {
        Color.cyan.opacity(0.12)
            .overlay {
                Image(systemName: "globe")
                    .font(.system(size: 36))
                    .foregroundStyle(Color.cyan.opacity(0.4))
            }
    }

    private var urlDomain: String {
        item.entry.parsedURL?.host ?? item.entry.urlWithoutScheme
    }

    // MARK: - Header / Bottom (dual style)

    private enum MetaStyle { case normal, overlay }

    private func headerRow(style: MetaStyle) -> some View {
        HStack {
            Text(item.entry.displayKind.localizedLabel)
                .font(.caption.weight(style == .overlay ? .medium : .regular))
                .foregroundStyle(style == .overlay ? AnyShapeStyle(.white) : AnyShapeStyle(.secondary))
            Spacer(minLength: 4)
            Text(item.timestamp.cardRelativeShort)
                .font(.caption)
                .foregroundStyle(style == .overlay ? AnyShapeStyle(.white.opacity(0.8)) : AnyShapeStyle(.tertiary))
        }
    }

    private func bottomRow(style: MetaStyle) -> some View {
        HStack(spacing: 4) {
            directionIcon
                .font(.caption2)
                .foregroundStyle(style == .overlay ? AnyShapeStyle(.white.opacity(0.7)) : AnyShapeStyle(.secondary))
            Spacer(minLength: 0)
            if isLatest {
                Circle()
                    .fill(style == .overlay ? .white : Color.accentColor)
                    .frame(width: 6, height: 6)
            }
        }
    }

    private var directionIcon: some View {
        Group {
            switch item.direction {
            case .pulled:
                Image(systemName: "arrow.down")
            case .pushed:
                Image(systemName: "arrow.up")
            case .local:
                Image(systemName: "internaldrive")
            }
        }
    }
}

// MARK: - Checkerboard background

/// Lightweight checkerboard pattern drawn in a Canvas. Used as the
/// letterbox fill on image cards so transparent PNGs don't float on void.
private struct CheckerboardBackground: View {
    var cellSize: CGFloat = 8
    var lightColor = Color(white: 0.85, opacity: 0.25)
    var darkColor = Color(white: 0.65, opacity: 0.25)

    var body: some View {
        Canvas { context, size in
            let cols = Int(ceil(size.width / cellSize))
            let rows = Int(ceil(size.height / cellSize))
            for row in 0..<rows {
                for col in 0..<cols {
                    let isLight = (row + col).isMultiple(of: 2)
                    let rect = CGRect(
                        x: CGFloat(col) * cellSize,
                        y: CGFloat(row) * cellSize,
                        width: cellSize,
                        height: cellSize
                    )
                    context.fill(
                        Path(rect),
                        with: .color(isLight ? lightColor : darkColor)
                    )
                }
            }
        }
    }
}

// MARK: - Relative time helper (card-specific)

private extension Date {
    var cardRelativeShort: String {
        let dt = timeIntervalSinceNow
        if abs(dt) < 5 { return String(localized: "刚刚") }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f.localizedString(for: self, relativeTo: .now)
    }
}

// MARK: - Previews

#Preview("Text card") {
    let item = ClipboardHistoryItem(
        entry: Clipboard(
            type: .text,
            hash: "AABB",
            text: "ssh alice@dev.home.lan -p 2222\nsome more content here to test multiline",
            hasData: false,
            size: 60
        ),
        timestamp: .now.addingTimeInterval(-45),
        direction: .pushed
    )
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ClipboardCard(item: item, isLatest: true)
        ClipboardCard(item: item, isLatest: false)
    }
    .padding()
    .background(Color(.systemGroupedBackground))
}

#Preview("Image card — with thumbnail") {
    let item = ClipboardHistoryItem(
        entry: Clipboard(
            type: .image,
            hash: "CCDD",
            text: "photo_2026.png",
            hasData: true,
            dataName: "photo_2026.png",
            size: 184_320
        ),
        timestamp: .now.addingTimeInterval(-3600),
        direction: .pulled
    )
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ClipboardCard(item: item, isLatest: false, thumbnailImage: nil)
        ClipboardCard(item: item, isLatest: true, thumbnailImage: UIImage(systemName: "photo.artframe"))
    }
    .padding()
    .background(Color(.systemGroupedBackground))
}

#Preview("Image card — local direction") {
    let item = ClipboardHistoryItem(
        entry: Clipboard(
            type: .image,
            hash: "EEFF",
            text: "screenshot.png",
            hasData: true,
            dataName: "screenshot.png",
            size: 512_000
        ),
        timestamp: .now.addingTimeInterval(-120),
        direction: .local
    )
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ClipboardCard(item: item, isLatest: true, thumbnailImage: UIImage(systemName: "photo.fill"))
        ClipboardCard(item: item, isLatest: false)
    }
    .padding()
    .background(Color(.systemGroupedBackground))
}

#Preview("File card") {
    let item = ClipboardHistoryItem(
        entry: Clipboard(
            type: .file,
            hash: "EEFF",
            text: "report.pdf",
            hasData: true,
            dataName: "report.pdf",
            size: 1_048_576
        ),
        timestamp: .now.addingTimeInterval(-2 * 86400),
        direction: .pulled
    )
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ClipboardCard(item: item, isLatest: false)
        ClipboardCard(item: item, isLatest: false, isLoading: true)
    }
    .padding()
    .background(Color(.systemGroupedBackground))
}

#Preview("URL card — with OG image") {
    let item = ClipboardHistoryItem(
        entry: Clipboard(
            type: .text,
            hash: "AABB",
            text: "https://github.com/anthropics/claude-code",
            hasData: false,
            size: 43
        ),
        timestamp: .now.addingTimeInterval(-300),
        direction: .pulled
    )
    let metadata = URLCardMetadata(
        title: "GitHub - anthropics/claude-code",
        ogImage: UIImage(systemName: "globe")
    )
    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        ClipboardCard(item: item, isLatest: true, urlMetadata: metadata)
        ClipboardCard(item: item, isLatest: false)
    }
    .padding()
    .background(Color(.systemGroupedBackground))
}
