#if canImport(UIKit)
import UIKit

/// Rasterizes a `ServerConfig` to a 56×56 circular monogram PNG.
///
/// Used by `ServerPersonFactory` for the `INPerson` avatar on Sharing
/// Suggestions tiles. iOS clips suggestion-tile images to a circle on
/// presentation, but we fill the whole square with the colored disc
/// anyway so the same bitmap renders correctly if anything ever
/// surfaces it square (e.g. Settings → "Allow Notifications From…").
///
/// Pure UIKit — no SwiftUI — so this file is safe inside the Share
/// Extension's app-extension SDK.
enum ServerAvatarRenderer {
    /// Side length in points. 56pt matches the share-suggestion tile size
    /// (the system asks for ~50pt; we go slightly larger so it stays crisp
    /// at @3x).
    private static let side: CGFloat = 56

    static func pngData(for server: ServerConfig) -> Data? {
        let initials = ServerAvatar.initials(for: server)
        let hue = CGFloat(ServerAvatar.hue(for: server))

        let renderer = UIGraphicsImageRenderer(
            size: CGSize(width: side, height: side),
            format: {
                let f = UIGraphicsImageRendererFormat.default()
                f.opaque = true
                f.scale = 3
                return f
            }()
        )

        let image = renderer.image { ctx in
            let rect = CGRect(x: 0, y: 0, width: side, height: side)
            let background = UIColor(hue: hue, saturation: 0.55, brightness: 0.85, alpha: 1.0)
            background.setFill()
            UIBezierPath(ovalIn: rect).fill()

            let attrs: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: side * 0.42, weight: .semibold),
                .foregroundColor: UIColor.white,
                .paragraphStyle: {
                    let p = NSMutableParagraphStyle()
                    p.alignment = .center
                    return p
                }(),
            ]
            let text = initials as NSString
            let size = text.size(withAttributes: attrs)
            let drawRect = CGRect(
                x: 0,
                y: (side - size.height) / 2,
                width: side,
                height: size.height
            )
            text.draw(in: drawRect, withAttributes: attrs)
            _ = ctx
        }
        return image.pngData()
    }
}
#endif
