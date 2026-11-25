import UIKit

@main
final class ClipboardFixtureDelegate: UIResponder, UIApplicationDelegate {}

@objc(ClipboardFixtureSceneDelegate)
final class ClipboardFixtureSceneDelegate: UIResponder, UIWindowSceneDelegate {
  var window: UIWindow?
  private let status = UILabel()

  func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
    guard let windowScene = scene as? UIWindowScene else { return }
    let viewController = UIViewController()
    viewController.view.backgroundColor = .systemBackground
    status.translatesAutoresizingMaskIntoConstraints = false
    viewController.view.addSubview(status)
    NSLayoutConstraint.activate([
      status.centerXAnchor.constraint(equalTo: viewController.view.centerXAnchor),
      status.centerYAnchor.constraint(equalTo: viewController.view.centerYAnchor),
    ])
    let window = UIWindow(windowScene: windowScene)
    window.rootViewController = viewController
    self.window = window
    window.makeKeyAndVisible()
  }

  func sceneDidBecomeActive(_ scene: UIScene) {
    UIPasteboard.general.string = "Unic Mobile E2E"
    status.text = "Clipboard fixture ready"
  }
}
