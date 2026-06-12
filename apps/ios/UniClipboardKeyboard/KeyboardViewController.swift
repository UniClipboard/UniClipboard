import UIKit
import SwiftUI

/// Principal class for the UniClip custom keyboard. iOS instantiates this
/// (`NSExtensionPrincipalClass = $(PRODUCT_MODULE_NAME).KeyboardViewController`)
/// when the user switches to the UniClip keyboard. It subclasses
/// `UIInputViewController` — the keyboard analog of a view controller — and
/// hosts a SwiftUI sheet via `UIHostingController`, mirroring the Share
/// Extension's `ShareViewController` approach.
///
/// The keyboard's job is clipboard *sync*, not text entry. On appear it:
///   1. reads the device pasteboard and pushes anything new to the active
///      server (**uplink** — "open keyboard = auto-sync"); and
///   2. pulls the server's latest clipboard and offers it as a one-tap
///      insert candidate (**downlink** — `insertText`, no pasteboard hop).
///
/// Both halves need **Full Access** (`RequestsOpenAccess=YES` + the user's
/// "允许完全访问" toggle): without it, `UIPasteboard` and `URLSession` are
/// both unavailable to a keyboard, so we render a "needs Full Access" hint
/// instead. The first content read after Full Access is granted fires iOS's
/// per-app "允许粘贴" prompt once; after the user allows, reads are silent —
/// which is exactly what makes the auto-sync feel automatic.
final class KeyboardViewController: UIInputViewController {
    private let model = KeyboardModel()
    private var host: UIHostingController<KeyboardRootView>?

    /// Custom keyboard height, sized to *hug* its content so the card row sits
    /// snug between the top bar and the key row instead of floating in a tall
    /// frame. The Paste-style layout stacks a branded/search top bar, a row of
    /// clipboard cards, and the space/⌫/return key row — that's
    /// `KeyboardLayout.contentHeight`, **computed from the same constants the
    /// SwiftUI layout consumes** (a hand-summed constant here once lagged a
    /// 2pt top-bar change and clipped the card row into looking like it had
    /// divider lines). The globe strip is added only when iOS needs an
    /// input-mode switch key (see `viewDidAppear`); without it the strip
    /// collapses and the keyboard shrinks by the same band, rather than letting
    /// the freed space float the cards up off the keys. Priority 999 (not
    /// required) so it can never conflict with the system-imposed constraints
    /// on the input view.
    private lazy var heightConstraint: NSLayoutConstraint = {
        let c = view.heightAnchor.constraint(equalToConstant: KeyboardLayout.contentHeight)
        c.priority = UILayoutPriority(999)
        return c
    }()

    override func viewDidLoad() {
        super.viewDidLoad()

        // Wire the model's UI callbacks to the input controller. `unowned`
        // is safe: the model is owned by (and outlived by) this controller.
        model.insertText = { [unowned self] text in
            self.textDocumentProxy.insertText(text)
        }
        model.deleteBackward = { [unowned self] in
            self.textDocumentProxy.deleteBackward()
        }
        model.advanceInputMode = { [unowned self] in
            self.advanceToNextInputMode()
        }
        model.dismiss = { [unowned self] in
            self.dismissKeyboard()
        }
        // The click only sounds when our input view adopts
        // `UIInputViewAudioFeedback` (below) and the user has 键盘点击音 on —
        // so the model can call this unconditionally and let iOS decide.
        model.playInputClick = { [unowned self] in
            UIDevice.current.playInputClick()
        }
        model.openSettings = { [unowned self] in
            let keyboard = URL(string: "App-prefs:General&path=Keyboard")
            let fallback = URL(string: UIApplication.openSettingsURLString)
            guard let url = keyboard ?? fallback else { return }
            let selector = sel_registerName("openURL:")
            var responder: UIResponder? = self
            while let r = responder {
                if r.responds(to: selector) {
                    r.perform(selector, with: url)
                    return
                }
                responder = r.next
            }
        }

        // Keep every layer *visually* transparent so the system-drawn keyboard
        // tray (flat gray pre-iOS 26, Liquid Glass on iOS 26+) shows through.
        // The controller's own view defaults opaque, and UIHostingController
        // adds an opaque background of its own — both would hide the system
        // tray and force us to fake a color that can't match Liquid Glass.
        //
        // But NOT alpha-zero: the system ignores touches that land on fully
        // transparent points of a custom keyboard (documented keyboard-window
        // behavior; see https://developer.apple.com/forums/thread/702798).
        // The quiet-chrome top bar is bare glyphs, so most of its hit area is
        // transparent pixels — with `.clear` here, taps beside the glyph
        // strokes were silently dropped before SwiftUI ever saw them. A
        // 0.001-alpha film is invisible over the tray but keeps every point
        // of the keyboard touch-opaque (same trick as the switcher scrim).
        view.backgroundColor = UIColor(white: 1, alpha: 0.001)

        let root = KeyboardRootView(model: model)
        let host = UIHostingController(rootView: root)
        self.host = host
        addChild(host)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        host.view.backgroundColor = .clear
        view.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        host.didMove(toParent: self)
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // `hasFullAccess` / `needsInputModeSwitchKey` are only reliable once
        // the input view is on screen — read them here, then drive the sync.
        model.needsInputModeSwitchKey = needsInputModeSwitchKey
        model.hasFullAccess = hasFullAccess
        // Size to content: add the globe band only when the strip is shown, so a
        // single-keyboard install doesn't leave the freed space floating the
        // card row up off the keys.
        heightConstraint.constant = needsInputModeSwitchKey
            ? KeyboardLayout.contentHeight + KeyboardLayout.stripBandHeight
            : KeyboardLayout.contentHeight
        heightConstraint.isActive = true
        model.setReturnKeyType(textDocumentProxy.returnKeyType)
        model.onAppear()
        if let group = UserDefaults(suiteName: SettingsStore.appGroupID) {
            group.set(true, forKey: AppSettings.PersistenceKey.keyboardExtensionEnabled)
            group.set(hasFullAccess, forKey: AppSettings.PersistenceKey.keyboardExtensionFullAccess)
        }
    }

    /// The host field can change (e.g. tapping from a search box to a body
    /// field) while our keyboard stays up. Re-read the Return-key intent so
    /// the key relabels itself (发送 / 搜索 / …) to match.
    override func textDidChange(_ textInput: (any UITextInput)?) {
        super.textDidChange(textInput)
        model.setReturnKeyType(textDocumentProxy.returnKeyType)
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        // Stop polling the pasteboard when the keyboard leaves the screen
        // (globe to another keyboard, dismissed, host app closed) so we don't
        // run a background timer the user can't see.
        model.stopMonitoring()
    }
}

/// Opt the keyboard into the system key-click sound. `playInputClick()` is a
/// no-op unless the first responder's input view (here, the input view
/// controller) adopts this protocol and returns `true`; iOS still gates the
/// actual sound on the user's global 键盘点击音 setting.
extension KeyboardViewController: UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}
