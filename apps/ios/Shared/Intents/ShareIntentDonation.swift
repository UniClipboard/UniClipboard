import Foundation
import Intents
import OSLog

private let log = Logger(subsystem: "app.uniclipboard", category: "intents")

/// Donates `INSendMessageIntent` interactions so iOS Sharing Suggestions
/// surfaces our servers as top-row "contact" tiles on the share sheet.
///
/// Why `INSendMessageIntent`: of the system intents that produce suggestion
/// tiles, only the messaging family treats `recipients` as discrete
/// entities the user picks between (vs. e.g. `INSendPaymentIntent` which
/// implies a single counterparty). The intent's semantics don't need to
/// match "real" messaging — Apple uses the same intent for any app that
/// "sends content to a recipient", including third-party clients like
/// Slack and WhatsApp.
///
/// Donation is fire-and-forget: errors are intentionally swallowed because
/// a failed donation only degrades the suggestion ranking, not the actual
/// upload that already succeeded.
public enum ShareIntentDonation {

    /// Record one "shared X to server Y" event. Call from the Share
    /// Extension *after* a successful upload.
    ///
    /// - Parameters:
    ///   - server: the destination server. Its `id` becomes the person
    ///     handle that ties future taps back to this `ServerConfig`.
    ///   - summary: short content description used as the intent's
    ///     `content` (e.g. the file name or a text snippet). Shown in
    ///     Spotlight / Sharing Suggestions previews; keep ≤ 100 chars.
    public static func donateSend(to server: ServerConfig, summary: String) async {
        let person = ServerPersonFactory.person(for: server)
        let intent = INSendMessageIntent(
            recipients: [person],
            outgoingMessageType: .outgoingMessageText,
            content: summary,
            speakableGroupName: INSpeakableString(spokenPhrase: server.displayLabel),
            conversationIdentifier: server.id,
            serviceName: "UniClipboard",
            sender: nil,
            attachments: nil
        )
        // Hint the system which side of the conversation this is. The
        // share extension is always the outgoing direction.
        intent.setImage(ServerPersonFactory.avatarImage(for: server), forParameterNamed: \.speakableGroupName)

        let interaction = INInteraction(intent: intent, response: nil)
        interaction.direction = .outgoing
        // groupIdentifier scopes the donation to a server so we can
        // wholesale-delete it when the user removes the server (see
        // `deleteAllDonations(forServerId:)`).
        interaction.groupIdentifier = server.id

        do {
            try await interaction.donate()
        } catch {
            // Best-effort. Don't surface to the user — the upload itself
            // already succeeded.
            log.warning("donateSend: donation failed: \(String(describing: error), privacy: .public)")
        }
    }

    /// Convenience: derive the user-facing summary from a `Clipboard`
    /// entry and forward to `donateSend(to:summary:)`. Used by the main
    /// app's SyncEngine when a routine push lands on the server, so the
    /// system's Sharing-Suggestions ranker treats "the user copied
    /// something and it auto-synced" the same as "the user explicitly
    /// shared via the system share sheet".
    public static func donateSend(to server: ServerConfig, clipboard: Clipboard) async {
        await donateSend(to: server, summary: summary(for: clipboard))
    }

    /// Drop every donation tied to `serverId`. Call this when the user
    /// deletes a server so iOS doesn't keep suggesting a dead destination
    /// on the share sheet.
    public static func deleteAllDonations(forServerId serverId: String) {
        INInteraction.delete(with: serverId) { error in
            if let error {
                log.warning("deleteAllDonations: delete failed: \(String(describing: error), privacy: .public)")
            }
        }
    }

    /// Same shape as `ShareItem.displayName` so the summary text iOS
    /// shows in Sharing-Suggestions previews is consistent regardless of
    /// whether the donation came from the Share Extension or the main
    /// app's SyncEngine.
    private static func summary(for c: Clipboard) -> String {
        switch c.type {
        case .text:
            let trimmed = c.text.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.count <= 80 { return trimmed }
            return String(trimmed.prefix(80)) + "…"
        case .image, .file:
            return c.dataName ?? c.text
        case .group:
            return c.text
        }
    }
}
