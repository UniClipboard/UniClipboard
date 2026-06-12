import Foundation
import SentryWithoutUIKit

/// Sentry SDK initialization for the Share Extension process. Mirrors the
/// main app's `UniClipboard/Diagnostics/SentryBootstrap.swift` but against
/// the `Sentry-WithoutUIKitOrAppKit` product — the full `Sentry` product's
/// swiftinterface trips APPLICATION_EXTENSION_API_ONLY on its UserFeedback
/// UIKit classes. Same privacy contract: no PII, no auto HTTP capture, no
/// clipboard content / filenames / server URLs in anything sent to Sentry.
enum SentryBootstrap {
    /// Start the SDK. Call from the principal class before any UI work.
    /// Idempotent; a no-op while `SentryDSN.value` is empty.
    static func start() {
        guard !SentryDSN.value.isEmpty, !SentrySDK.isEnabled else { return }
        SentrySDK.start { options in
            options.dsn = SentryDSN.value
            #if DEBUG
            options.environment = "development"
            #else
            options.environment = "production"
            #endif

            options.sendDefaultPii = false
            options.enableCaptureFailedRequests = false

            options.enableLogs = true
            options.beforeSendLog = { log in
                #if !DEBUG
                if log.level == .trace || log.level == .debug { return nil }
                #endif
                return log
            }

            // Short-lived process with a tight memory budget — error
            // monitoring + logs only.
            options.tracesSampleRate = 0
            options.enableAppHangTracking = false
            options.enableWatchdogTerminationTracking = false
        }
        SentrySDK.configureScope { scope in
            scope.setTag(value: "share-extension", key: "process")
        }
    }
}
