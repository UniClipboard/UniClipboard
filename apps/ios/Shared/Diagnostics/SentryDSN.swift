import Foundation

/// The one shared piece of the Sentry setup. The actual `SentrySDK.start`
/// bootstraps are per-target (`UniClipboard/Diagnostics/SentryBootstrap.swift`,
/// `UniClipboardShare/SentryBootstrap.swift`) — they can't live in
/// `Shared/` because the keyboard extension and the SwiftPM test targets
/// compile `Shared/` without linking any Sentry product, and
/// `#if canImport(...)` is no protection: every framework in the shared
/// BUILT_PRODUCTS_DIR is import-able from every target, linked or not.
///
/// Both targets link `Sentry-WithoutUIKitOrAppKit` (module
/// `SentryWithoutUIKit`), NOT the standard `Sentry` product:
/// - the full product's swiftinterface trips APPLICATION_EXTENSION_API_ONLY
///   in the Share Extension on its UserFeedback UIKit classes, and
/// - mixing the two variants in one project breaks outright — both
///   frameworks land in the same products dir and their ObjC headers
///   cross-include via `__has_include`, producing redefinition errors.
/// The trade (no screenshot/replay/UIKit auto-instrumentation) is exactly
/// the feature set this app must keep disabled anyway.
enum SentryDSN {
    /// Public DSN — not a secret (it ships in the binary anyway). Setting
    /// it to the empty string disables the SDK entirely.
    static let value = "https://5732af6025348f532d44b16b37981b8e@o404286.ingest.us.sentry.io/4511547584544768"
}
