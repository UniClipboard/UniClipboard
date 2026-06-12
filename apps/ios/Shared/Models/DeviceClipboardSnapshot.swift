import Foundation

/// Bytes-fresh read of the device pasteboard. The clipboard metadata for
/// UI display + the raw payload bytes for the push path. Payload is
/// `nil` for short text (everything is in `clipboard.text` already) and
/// non-nil for long text and images.
///
/// Lives in `Shared/` (not the app-only `DevicePasteboardObserver`) so the
/// Share Extension and the Widget Extension — which both run the §3.5
/// push sequence through `SendClipboardIntent` — can name the same type
/// without dragging in the UIKit-bound observer. Pure Foundation: the
/// SwiftPM `UniClipboardModels` target compiles it without UIKit.
struct DeviceClipboardSnapshot {
    let clipboard: Clipboard
    let payload: Data?
}
