import SwiftUI

extension AppearanceMode {
    /// `nil` means "follow system" — the value `preferredColorScheme(_:)`
    /// expects to release the override.
    var colorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light:  return .light
        case .dark:   return .dark
        }
    }

    var localizedLabel: String {
        switch self {
        case .system: return String(localized: "跟随系统")
        case .light:  return String(localized: "浅色")
        case .dark:   return String(localized: "深色")
        }
    }
}
