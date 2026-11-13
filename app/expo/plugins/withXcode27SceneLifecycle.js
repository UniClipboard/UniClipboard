"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.patchAppDelegateForXcode27 = void 0;
const config_plugins_1 = require("expo/config-plugins");
const LEGACY_WINDOW_STARTUP = `#if os(iOS) || os(tvOS)
    window = UIWindow(frame: UIScreen.main.bounds)
    factory.startReactNative(
      withModuleName: "main",
      in: window,
      launchOptions: launchOptions)
#endif`;
const TVOS_WINDOW_STARTUP = `#if os(tvOS)
    window = UIWindow(frame: UIScreen.main.bounds)
    factory.startReactNative(
      withModuleName: "main",
      in: window,
      launchOptions: launchOptions)
#endif`;
const SCENE_DELEGATE = `@objc(SceneDelegate)
class SceneDelegate: UIResponder, UIWindowSceneDelegate {
  var window: UIWindow?

  private var appDelegate: AppDelegate? {
    UIApplication.shared.delegate as? AppDelegate
  }

  func scene(
    _ scene: UIScene,
    willConnectTo session: UISceneSession,
    options connectionOptions: UIScene.ConnectionOptions
  ) {
    guard let windowScene = scene as? UIWindowScene else {
      return
    }
    guard let appDelegate, let factory = appDelegate.reactNativeFactory else {
      fatalError("SceneDelegate could not access the React Native factory")
    }

    let window = UIWindow(windowScene: windowScene)
    self.window = window
    appDelegate.window = window

    let browsingWebActivity = connectionOptions.userActivities.first {
      $0.activityType == NSUserActivityTypeBrowsingWeb
    }
    factory.startReactNative(
      withModuleName: "main",
      in: window,
      launchOptions: Self.launchOptions(
        url: connectionOptions.urlContexts.first?.url,
        userActivity: browsingWebActivity
      )
    )

    Self.route(urlContexts: connectionOptions.urlContexts)
    connectionOptions.userActivities.forEach { Self.route(userActivity: $0) }
  }

  func sceneDidDisconnect(_ scene: UIScene) {
    if appDelegate?.window === window {
      appDelegate?.window = nil
    }
    window = nil
  }

  func sceneDidBecomeActive(_ scene: UIScene) {
    appDelegate?.applicationDidBecomeActive(UIApplication.shared)
  }

  func sceneWillResignActive(_ scene: UIScene) {
    appDelegate?.applicationWillResignActive(UIApplication.shared)
  }

  func sceneWillEnterForeground(_ scene: UIScene) {
    appDelegate?.applicationWillEnterForeground(UIApplication.shared)
  }

  func sceneDidEnterBackground(_ scene: UIScene) {
    appDelegate?.applicationDidEnterBackground(UIApplication.shared)
  }

  func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) {
    Self.route(urlContexts: URLContexts)
  }

  func scene(_ scene: UIScene, continue userActivity: NSUserActivity) {
    Self.route(userActivity: userActivity)
  }

  private static func launchOptions(
    url: URL?,
    userActivity: NSUserActivity?
  ) -> [UIApplication.LaunchOptionsKey: Any]? {
    var launchOptions: [UIApplication.LaunchOptionsKey: Any] = [:]
    if let url {
      let urlKey = UIApplication.LaunchOptionsKey(rawValue: "UIApplicationLaunchOptionsURLKey")
      launchOptions[urlKey] = url
    }
    if let userActivity {
      let userActivityDictionaryKey = UIApplication.LaunchOptionsKey(
        rawValue: "UIApplicationLaunchOptionsUserActivityDictionaryKey"
      )
      launchOptions[userActivityDictionaryKey] = [
        "UIApplicationLaunchOptionsUserActivityTypeKey": userActivity.activityType,
        "UIApplicationLaunchOptionsUserActivityKey": userActivity,
      ]
    }
    return launchOptions.isEmpty ? nil : launchOptions
  }

  private static func route(urlContexts: Set<UIOpenURLContext>) {
    guard let appDelegate = UIApplication.shared.delegate as? AppDelegate else {
      return
    }
    for context in urlContexts {
      _ = appDelegate.application(
        UIApplication.shared,
        open: context.url,
        options: openURLOptions(from: context.options)
      )
    }
  }

  private static func route(userActivity: NSUserActivity) {
    guard let appDelegate = UIApplication.shared.delegate as? AppDelegate else {
      return
    }
    _ = appDelegate.application(
      UIApplication.shared,
      continue: userActivity,
      restorationHandler: { _ in }
    )
  }

  private static func openURLOptions(
    from sceneOptions: UIScene.OpenURLOptions
  ) -> [UIApplication.OpenURLOptionsKey: Any] {
    var options: [UIApplication.OpenURLOptionsKey: Any] = [:]
    if let sourceApplication = sceneOptions.sourceApplication {
      options[.sourceApplication] = sourceApplication
    }
    if let annotation = sceneOptions.annotation {
      options[.annotation] = annotation
    }
    options[.openInPlace] = sceneOptions.openInPlace
    return options
  }
}`;
const REACT_NATIVE_DELEGATE_ANCHOR = '\nclass ReactNativeDelegate:';
const patchAppDelegateForXcode27 = (contents) => {
    if (contents.includes(SCENE_DELEGATE)) {
        return contents;
    }
    const windowStartupOccurrences = contents.split(LEGACY_WINDOW_STARTUP).length - 1;
    const delegateAnchorOccurrences = contents.split(REACT_NATIVE_DELEGATE_ANCHOR).length - 1;
    if (windowStartupOccurrences !== 1 || delegateAnchorOccurrences !== 1) {
        throw new Error(`Unsupported Expo AppDelegate.swift: expected one window startup and one ReactNativeDelegate anchor, found ${windowStartupOccurrences} and ${delegateAnchorOccurrences}`);
    }
    return contents
        .replace(LEGACY_WINDOW_STARTUP, TVOS_WINDOW_STARTUP)
        .replace(REACT_NATIVE_DELEGATE_ANCHOR, `\n${SCENE_DELEGATE}\n\nclass ReactNativeDelegate:`);
};
exports.patchAppDelegateForXcode27 = patchAppDelegateForXcode27;
const withXcode27SceneLifecycle = (config) => (0, config_plugins_1.withAppDelegate)(config, (config) => {
    if (config.modResults.language !== 'swift') {
        throw new Error('withXcode27SceneLifecycle requires a Swift AppDelegate');
    }
    config.modResults.contents = (0, exports.patchAppDelegateForXcode27)(config.modResults.contents);
    return config;
});
exports.default = (0, config_plugins_1.createRunOncePlugin)(withXcode27SceneLifecycle, 'withXcode27SceneLifecycle', '1.0.0');
