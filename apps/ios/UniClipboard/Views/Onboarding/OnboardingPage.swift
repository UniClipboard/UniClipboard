import SwiftUI

/// One screen in the onboarding walkthrough. The ordered page arrays below are
/// the single source of truth for page count + order — the container
/// (`OnboardingView`) picks one per `Mode` and the page indicator reads its
/// length, so adding a page is a one-line change here.
///
/// `feature` = a Symbol 卖点页 (welcome / auto-sync / multi-server).
/// `tutorial` = a 截图教学页 (keyboard / share extension / paste permission)
/// that raises a how-to sheet for a system action the app can't perform itself.
enum OnboardingPage: Identifiable, Hashable {
    case feature(Feature)
    case tutorial(Tutorial)

    var id: String {
        switch self {
        case .feature(let f):  return "feature.\(f.rawValue)"
        case .tutorial(let t): return "tutorial.\(t.rawValue)"
        }
    }

    /// The 3 Symbol 卖点页, in order. This is the entire **first-run** sequence
    /// (`Mode.firstRun`): pairing-first redesign — the walkthrough now only
    /// sells the product, then hands straight off to the server SetupFlow. The
    /// 教学页 moved to the post-pairing carousel (see `tutorials`).
    static let features: [OnboardingPage] = [
        .feature(.crossPlatform),
        .feature(.richAccess),
        .feature(.openSource),
    ]

    /// The 3 截图教学页, in order (keyboard → share → paste-permission). Shown
    /// as the post-pairing "解锁更多" carousel (`Mode.enhancements`), and again
    /// as the tail of the Settings re-view (`Mode.review`). 快捷指令 & 小组件
    /// are not given their own pages by design.
    static let tutorials: [OnboardingPage] = [
        .tutorial(.keyboard),
        .tutorial(.shareExtension),
        .tutorial(.pastePermission),
    ]

    /// Full sequence — 卖点 then 教学. Used only by the Settings re-view
    /// (`Mode.review`), where the user wants the complete tour minus pairing.
    static let all: [OnboardingPage] = features + tutorials
}

// MARK: - Feature (Symbol 卖点页)

extension OnboardingPage {
    enum Feature: String, Hashable {
        case crossPlatform
        case richAccess
        case openSource

        var title: LocalizedStringKey {
            switch self {
            case .crossPlatform: return "你的跨平台剪贴板"
            case .richAccess:    return "分享、键盘、随处可达"
            case .openSource:    return "开源自托管\n数据由你掌控"
            }
        }

        var subtitle: LocalizedStringKey {
            switch self {
            case .crossPlatform:
                return "Mac、Windows、Linux、iOS——复制一处，处处可用。不再被单一平台锁定。"
            case .richAccess:
                return "从任意 App 分享内容到服务器，用键盘一键粘贴到任意输入框——你的剪贴板，随处可达。"
            case .openSource:
                return "部署在自己的服务器上，数据不经过任何第三方。完全开源，代码透明可审计。"
            }
        }
    }
}

// MARK: - Tutorial (设备框教学页)

extension OnboardingPage {
    enum Tutorial: String, Hashable {
        case shareExtension
        case keyboard
        case pastePermission

        /// Asset-catalog name of the full-bleed hero screenshot for this page.
        var heroImageName: String {
            switch self {
            case .shareExtension:  return "OnboardingShare"
            case .keyboard:        return "OnboardingKeyboard"
            case .pastePermission: return "OnboardingPaste"
            }
        }

        /// Which half of the device the hero frames. `.top` (rounded top +
        /// Dynamic Island, content reads top-down) for the share flow; `.bottom`
        /// (rounded bottom, UI docks at the screen bottom) for the keyboard and
        /// the settings/paste page.
        var frameEdge: PhoneFrame.Edge {
            switch self {
            case .shareExtension:  return .top
            case .keyboard:        return .bottom
            case .pastePermission: return .bottom
            }
        }

        /// Which slice of the full-screen capture to scroll into the phone
        /// window: 0 = top, 0.5 = middle, 1 = bottom. Share is dialed to the
        /// middle so the share sheet's UniClip app row (the teaching focus)
        /// sits in view; the keyboard docks at the bottom; the settings rows
        /// sit at the top. Tune per-page when the exact position matters.
        var heroAnchor: CGFloat {
            switch self {
            case .shareExtension:  return 0.5
            case .keyboard:        return 1.0
            case .pastePermission: return 0.75
            }
        }

        /// Gap on the phone frame's rounded edge (opposite the shear). The
        /// keyboard seats its rounded bottom flush against the hero box (0); the
        /// others use the default `inset`. nil → default.
        var heroFramedEdgeInset: CGFloat? {
            switch self {
            case .keyboard:        return 0
            case .pastePermission: return 0
            default:               return nil
            }
        }

        var title: LocalizedStringKey {
            switch self {
            case .shareExtension:  return "从任何 App 分享进来"
            case .keyboard:        return "在任意 App 里用键盘粘贴"
            case .pastePermission: return "让粘贴不再被打断"
            }
        }

        var subtitle: LocalizedStringKey {
            switch self {
            case .shareExtension:
                return "在任意 App 点系统「分享」选 UniClip,链接、文本、图片就能直接同步到服务器。把它收藏到分享菜单顶部,用起来更顺手。"
            case .keyboard:
                return "切换到 UniClip 键盘,本机复制的内容会自动同步,服务器最新的内容点一下即可插入——全程不弹「允许粘贴」。"
            case .pastePermission:
                return "iOS 默认每次读取其他 App 复制的内容都会询问。把「从其他 App 粘贴」设为「允许」,UniClip 的同步就能完全静默。"
            }
        }

        /// 介绍页主按钮文案 — raises the how-to sheet.
        var primaryCTA: LocalizedStringKey {
            switch self {
            case .shareExtension:  return "设置分享扩展"
            case .keyboard:        return "启用 UniClip 键盘"
            case .pastePermission: return "开始设置"
            }
        }

        var howTo: HowToContent {
            switch self {
            case .shareExtension:
                return HowToContent(
                    title: "通过分享同步到服务器",
                    steps: [
                        "在任意 App 中选中文字、图片或文件,点「分享」",
                        "在分享面板中选择 UniClip",
                        "确认目标服务器,点「发送」即可同步",
                        "收藏 UniClip 到分享菜单顶部,下次更快找到",
                    ],
                    heroImage: nil,
                    footnote: "支持文本、链接、图片和文件。内容会直接上传到当前选中的服务器。",
                    primaryAction: HowToAction(title: "打开分享菜单试试", kind: .openShareSheet),
                    secondaryAction: nil
                )
            case .keyboard:
                return HowToContent(
                    title: "如何启用 UniClip 键盘?",
                    steps: [
                        "设置 › 通用 › 键盘 › 键盘 › 添加新键盘,选择 UniClip",
                        "点按 UniClip,打开「允许完全访问」",
                        "复制新内容后首次会弹一次「允许粘贴」,允许即可",
                    ],
                    heroImage: nil,
                    footnote: "「允许完全访问」用于联网同步。系统未提供直达键盘设置的链接,需从系统设置逐级进入。",
                    primaryAction: HowToAction(title: "去设置", kind: .openKeyboardSettings),
                    secondaryAction: nil
                )
            case .pastePermission:
                return HowToContent(
                    title: "如何允许从其他 App 粘贴?",
                    steps: [
                        "点「从其他 App 粘贴」",
                        "选择「允许」",
                    ],
                    heroImage: nil,
                    footnote: nil,
                    primaryAction: HowToAction(title: "打开设置", kind: .openSettings),
                    secondaryAction: nil
                )
            }
        }
    }
}
