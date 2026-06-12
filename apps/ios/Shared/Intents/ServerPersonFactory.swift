import Foundation
import Intents
#if canImport(UIKit)
import UIKit
#endif

/// Bridges a `ServerConfig` to the `INPerson` shape Sirikit's Sharing
/// Suggestions infrastructure expects. Each server becomes one "person";
/// once we `INInteraction.donate()` against that person, the system
/// surfaces it as a top-row contact tile on the share sheet — same row
/// where Messages threads and Mail contacts appear.
///
/// The choice of `personHandle.value = server.id` is load-bearing: when
/// the user later taps a suggested tile, iOS hands the original intent
/// back to our Share Extension, and we use that handle value to look up
/// which `ServerConfig` to upload to (see `ShareViewController`).
///
/// Note: this file deliberately lives outside `Shared/Models/` so the
/// SwiftPM packages don't have to link `Intents.framework`. The two Xcode
/// targets pick it up via their synchronized root group.
public enum ServerPersonFactory {

    /// Construct the `INPerson` that represents `server` on the share sheet.
    /// The `image` parameter slot is set on the returned person so the
    /// suggestion tile shows a colored monogram rather than the generic
    /// gray silhouette.
    public static func person(for server: ServerConfig) -> INPerson {
        let handle = INPersonHandle(value: server.id, type: .unknown)
        let person = INPerson(
            personHandle: handle,
            nameComponents: nil,
            displayName: server.displayLabel,
            image: avatarImage(for: server),
            contactIdentifier: nil,
            customIdentifier: server.id
        )
        return person
    }

    /// Render the monogram avatar to an `INImage`. Returns `nil` on
    /// platforms without UIKit (notably the SwiftPM test harness on macOS),
    /// in which case the suggestion tile falls back to the system
    /// silhouette — acceptable; the donation still works.
    public static func avatarImage(for server: ServerConfig) -> INImage? {
        #if canImport(UIKit)
        guard let data = ServerAvatarRenderer.pngData(for: server) else { return nil }
        return INImage(imageData: data)
        #else
        return nil
        #endif
    }
}
