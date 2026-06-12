import SwiftUI

/// Color + glyph + label for one Clipboard.Kind. Used in cards and history rows.
struct ClipboardKindBadge: View {
    enum Size { case small, medium, large }

    let kind: ClipboardDisplayKind
    var size: Size = .medium
    var showsLabel: Bool = true

    var body: some View {
        HStack(spacing: size.spacing) {
            Image(systemName: kind.symbolName)
                .font(size.glyphFont)
                .foregroundStyle(.white)
                .frame(width: size.iconBox, height: size.iconBox)
                .background(kind.tint.gradient,
                            in: RoundedRectangle(cornerRadius: size.iconCorner, style: .continuous))

            if showsLabel {
                Text(kind.localizedLabel)
                    .font(size.labelFont)
                    .foregroundStyle(.primary)
            }
        }
    }
}

extension Clipboard.Kind {
    var symbolName: String {
        switch self {
        case .text:  "doc.text.fill"
        case .image: "photo.fill"
        case .file:  "doc.fill"
        case .group: "folder.fill"
        }
    }

    var tint: Color {
        switch self {
        case .text:  .blue
        case .image: .green
        case .file:  .orange
        case .group: .purple
        }
    }

    var localizedLabel: LocalizedStringKey {
        switch self {
        case .text:  "文本"
        case .image: "图片"
        case .file:  "文件"
        case .group: "归档"
        }
    }
}

private extension ClipboardKindBadge.Size {
    var iconBox: CGFloat {
        switch self { case .small: 22; case .medium: 30; case .large: 56 }
    }
    var iconCorner: CGFloat {
        switch self { case .small: 6; case .medium: 8; case .large: 14 }
    }
    var glyphFont: Font {
        switch self { case .small: .caption; case .medium: .footnote; case .large: .title }
    }
    var labelFont: Font {
        switch self { case .small: .caption; case .medium: .subheadline.weight(.semibold); case .large: .headline }
    }
    var spacing: CGFloat {
        switch self { case .small: 6; case .medium: 8; case .large: 12 }
    }
}

#Preview("Badges") {
    VStack(alignment: .leading, spacing: 16) {
        ForEach(ClipboardDisplayKind.allCases, id: \.self) { kind in
            HStack(spacing: 16) {
                ClipboardKindBadge(kind: kind, size: .small)
                ClipboardKindBadge(kind: kind, size: .medium)
                ClipboardKindBadge(kind: kind, size: .large)
            }
        }
    }
    .padding()
}
