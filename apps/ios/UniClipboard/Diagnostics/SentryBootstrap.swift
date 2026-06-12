import Foundation
import SentryWithoutUIKit

/// Sentry SDK initialization for the main app process. The Share
/// Extension has its own copy — see the note on `SentryDSN` for why this
/// can't live in `Shared/`.
///
/// Privacy contract (this is a clipboard app — content is radioactive):
/// - never enable Session Replay, screenshots, or view-hierarchy capture;
///   any of them would upload the user's clipboard content.
/// - never log clipboard text, filenames, hashes, or server URLs to
///   Sentry. Attributes are limited to error kinds, entry types, byte
///   counts, and tick counters. Full detail stays in OSLog on-device.
/// - `enableCaptureFailedRequests` stays off — the auto-captured HTTP
///   events would carry the user's self-hosted server address.
enum SentryBootstrap {
    /// Start the SDK. Call as the first thing in the process (app `init`)
    /// so the crash handler covers the whole launch. Idempotent; a no-op
    /// while `SentryDSN.value` is empty.
    static func start() {
        guard !SentryDSN.value.isEmpty, !SentrySDK.isEnabled else { return }
        SentrySDK.start { options in
            options.dsn = SentryDSN.value
            #if DEBUG
            options.environment = "development"
            #else
            options.environment = "production"
            #endif

            // Privacy — see the type-level contract above. Screenshot /
            // view-hierarchy / replay options don't exist in the
            // WithoutUIKit build, which is itself the strongest guarantee
            // they stay off.
            options.sendDefaultPii = false
            options.enableCaptureFailedRequests = false

            // Structured logs (Sentry Logs page). Debug/trace stay
            // on-device in Release builds.
            options.enableLogs = true
            options.beforeSendLog = { log in
                #if !DEBUG
                if log.level == .trace || log.level == .debug { return nil }
                #endif
                return log
            }

            options.tracesSampleRate = 0.1
        }
        SentrySDK.configureScope { scope in
            scope.setTag(value: "app", key: "process")
        }
    }
}
