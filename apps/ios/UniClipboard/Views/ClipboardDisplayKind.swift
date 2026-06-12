import SwiftUI

/// UI-layer display kind that extends the wire `Clipboard.Kind` with
/// client-detected types like `.url`. The wire protocol stays untouched;
/// this enum drives rendering, filtering, and badge appearance only.
enum ClipboardDisplayKind: CaseIterable, Hashable {
    case text, url, image, file, group

    var symbolName: String {
        switch self {
        case .text:  "doc.text.fill"
        case .url:   "link"
        case .image: "photo.fill"
        case .file:  "doc.fill"
        case .group: "folder.fill"
        }
    }

    var tint: Color {
        switch self {
        case .text:  .blue
        case .url:   .cyan
        case .image: .green
        case .file:  .orange
        case .group: .purple
        }
    }

    var localizedLabel: LocalizedStringKey {
        switch self {
        case .text:  "文本"
        case .url:   "链接"
        case .image: "图片"
        case .file:  "文件"
        case .group: "归档"
        }
    }

    var localizedLabelString: String {
        switch self {
        case .text:  String(localized: "文本")
        case .url:   String(localized: "链接")
        case .image: String(localized: "图片")
        case .file:  String(localized: "文件")
        case .group: String(localized: "归档")
        }
    }
}

extension Clipboard {
    var displayKind: ClipboardDisplayKind {
        switch type {
        case .text:  return isURL ? .url : .text
        case .image: return .image
        case .file:  return .file
        case .group: return .group
        }
    }

    var isURL: Bool {
        guard type == .text, !hasData else { return false }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.contains(where: \.isNewline),
              let url = URL(string: trimmed),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              url.host != nil
        else { return false }
        return true
    }

    var parsedURL: URL? {
        guard isURL else { return nil }
        return URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    var urlWithoutScheme: String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if let range = trimmed.range(of: "://") {
            return String(trimmed[range.upperBound...])
        }
        return trimmed
    }
}
