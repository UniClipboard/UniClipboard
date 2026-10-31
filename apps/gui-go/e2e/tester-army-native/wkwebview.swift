import AppKit
import WebKit

// No window, persistent website store, clipboard access, or activation.
NSApplication.shared.setActivationPolicy(.prohibited)
let configuration = WKWebViewConfiguration()
configuration.websiteDataStore = .nonPersistent()
let view = WKWebView(frame: .zero, configuration: configuration)
view.isInspectable = true
final class Navigation: NSObject, WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        webView.evaluateJavaScript("document.getElementById('probe').textContent") { value, error in
            guard error == nil, value as? String == "isolated WKWebView" else { exit(1) }
            print("{\"nativeRuntime\":\"WKWebView\",\"ephemeralStore\":true,\"inspectable\":true,\"javascriptReadback\":true,\"testerArmyAcceptance\":false}")
            exit(0)
        }
    }
}
let navigation = Navigation()
view.navigationDelegate = navigation
view.loadHTMLString("<p id='probe'>isolated WKWebView</p>", baseURL: nil)
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { exit(2) }
RunLoop.main.run()
