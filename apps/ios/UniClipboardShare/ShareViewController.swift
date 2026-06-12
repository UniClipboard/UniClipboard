import UIKit
import SwiftUI
import Intents

/// Principal class for the Share Extension. iOS instantiates this when the
/// user picks UniClipboard from the system share sheet. We don't subclass
/// `SLComposeServiceViewController` — that gives a Twitter-style compose
/// UI that doesn't fit our flow (server picker + content preview + send).
/// Instead we host our own SwiftUI sheet inside a plain `UIViewController`.
final class ShareViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()

        SentryBootstrap.start()

        let context = self.extensionContext
        // If the user tapped a Sharing-Suggestions tile (the "contact" row
        // we donate via INSendMessageIntent), iOS hands us the original
        // intent back through `extensionContext.intent`. The first
        // recipient's personHandle.value is the server id we stamped on
        // the donation, so the SwiftUI layer can fast-path to "uploading"
        // without showing the picker.
        let prefilledServerId: String? = {
            guard let sendMessage = context?.intent as? INSendMessageIntent,
                  let handle = sendMessage.recipients?.first?.personHandle?.value,
                  !handle.isEmpty else { return nil }
            return handle
        }()

        let root = ShareRootView(
            context: context.map(ShareExtensionContext.init),
            prefilledServerId: prefilledServerId,
            onFinish: { [weak self] in
                self?.extensionContext?.completeRequest(returningItems: nil, completionHandler: nil)
            },
            onCancel: { [weak self] in
                self?.extensionContext?.cancelRequest(withError: NSError(
                    domain: NSCocoaErrorDomain,
                    code: NSUserCancelledError
                ))
            }
        )

        let host = UIHostingController(rootView: root)
        addChild(host)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
        host.didMove(toParent: self)
    }
}

/// Thin wrapper around `NSExtensionContext` so the SwiftUI view doesn't
/// reach into UIKit for attachment loading. Bridges the host VC's context
/// into the value-typed world; the view holds it as an `Optional` so the
/// SwiftUI preview can pass `nil`.
struct ShareExtensionContext {
    let inputItems: [NSExtensionItem]

    init(_ context: NSExtensionContext) {
        self.inputItems = context.inputItems.compactMap { $0 as? NSExtensionItem }
    }
}
