import UIKit

enum KeyboardLayoutMetrics {
  static let isPad = UIDevice.current.userInterfaceIdiom == .pad

  static let outerPaddingTop: CGFloat = 8
  static let outerPaddingHorizontal: CGFloat = 8
  static let outerPaddingBottom: CGFloat = 5
  static let sectionGap: CGFloat = 7

  static let railWidth: CGFloat = 48
  static let railButtonSize = CGSize(width: 40, height: 39)
  static let editBarHeight: CGFloat = 38
  static let globeStripHeight: CGFloat = isPad ? 38 : 34

  /// Primary C1 keyboard content height (matches finalized C1 prototype: 302pt).
  static let normalContentHeight: CGFloat = isPad ? 326 : 302
  /// Compact height when Full Access has not been granted yet.
  static let restrictedContentHeight: CGFloat = 228

  static func targetHeight(hasFullAccess: Bool, needsInputModeSwitchKey: Bool) -> CGFloat {
    let base = hasFullAccess ? normalContentHeight : restrictedContentHeight
    return base + (needsInputModeSwitchKey ? globeStripHeight : 0)
  }
}

enum KeyboardTheme {
  static func dynamic(light: UIColor, dark: UIColor) -> UIColor {
    UIColor { trait in
      trait.userInterfaceStyle == .dark ? dark : light
    }
  }

  /// unic v2 signature monochrome ink accent (#15171C light / #F4F2EE dark).
  static let accent = dynamic(
    light: UIColor(red: 0.082, green: 0.090, blue: 0.110, alpha: 1),
    dark: UIColor(red: 0.957, green: 0.949, blue: 0.933, alpha: 1)
  )

  static let onAccent = dynamic(
    light: .white,
    dark: UIColor(red: 0.082, green: 0.090, blue: 0.110, alpha: 1)
  )

  static let railBackground = dynamic(
    light: UIColor.white.withAlphaComponent(0.68),
    dark: UIColor(white: 0.18, alpha: 0.68)
  )

  static let cardBackground = dynamic(
    light: UIColor.white.withAlphaComponent(0.94),
    dark: UIColor(white: 0.17, alpha: 0.92)
  )

  static let cardSpotlightBackground = dynamic(
    light: UIColor(red: 0.980, green: 0.984, blue: 0.996, alpha: 0.98),
    dark: UIColor(red: 0.20, green: 0.205, blue: 0.225, alpha: 0.96)
  )

  static let cardOTPBackground = dynamic(
    light: UIColor(red: 0.968, green: 0.945, blue: 0.996, alpha: 0.96),
    dark: UIColor(red: 0.22, green: 0.17, blue: 0.27, alpha: 0.94)
  )

  static let cardBorder = dynamic(
    light: UIColor.black.withAlphaComponent(0.07),
    dark: UIColor.white.withAlphaComponent(0.10)
  )

  static let spotlightBorder = dynamic(
    light: UIColor(red: 0.082, green: 0.090, blue: 0.110, alpha: 0.20),
    dark: UIColor(red: 0.957, green: 0.949, blue: 0.933, alpha: 0.26)
  )

  static let otpBorder = dynamic(
    light: UIColor(red: 0.576, green: 0.200, blue: 0.918, alpha: 0.28),
    dark: UIColor(red: 0.753, green: 0.518, blue: 0.988, alpha: 0.34)
  )

  static let keyBackground = dynamic(
    light: UIColor.white.withAlphaComponent(0.95),
    dark: UIColor(white: 0.24, alpha: 1.0)
  )

  static let keyMutedBackground = dynamic(
    light: UIColor.white.withAlphaComponent(0.58),
    dark: UIColor.white.withAlphaComponent(0.13)
  )

  static let keyActiveBackground = dynamic(
    light: UIColor(red: 0.89, green: 0.905, blue: 0.94, alpha: 1),
    dark: UIColor(white: 0.33, alpha: 1)
  )

  static let fill = dynamic(
    light: UIColor(red: 0.47, green: 0.47, blue: 0.50, alpha: 0.12),
    dark: UIColor(red: 0.47, green: 0.47, blue: 0.50, alpha: 0.26)
  )

  static let separator = dynamic(
    light: UIColor(red: 0.235, green: 0.235, blue: 0.263, alpha: 0.14),
    dark: UIColor.white.withAlphaComponent(0.11)
  )

  static let glassOverlayBackground = dynamic(
    light: UIColor.white.withAlphaComponent(0.94),
    dark: UIColor(white: 0.18, alpha: 0.95)
  )

  static let tintText = dynamic(light: UIColor(red: 0.0, green: 0.478, blue: 1.0, alpha: 1), dark: UIColor(red: 0.039, green: 0.518, blue: 1.0, alpha: 1))
  static let tintURL = dynamic(light: UIColor(red: 0.008, green: 0.518, blue: 0.780, alpha: 1), dark: UIColor(red: 0.220, green: 0.741, blue: 0.973, alpha: 1))
  static let tintImage = dynamic(light: UIColor(red: 0.141, green: 0.541, blue: 0.239, alpha: 1), dark: UIColor(red: 0.188, green: 0.820, blue: 0.345, alpha: 1))
  static let tintOTP = dynamic(light: UIColor(red: 0.576, green: 0.200, blue: 0.918, alpha: 1), dark: UIColor(red: 0.753, green: 0.518, blue: 0.988, alpha: 1))
  static let tintPin = dynamic(light: UIColor(red: 0.961, green: 0.620, blue: 0.043, alpha: 1), dark: UIColor(red: 0.984, green: 0.749, blue: 0.141, alpha: 1))
  static let tintGreen = dynamic(light: UIColor(red: 0.204, green: 0.780, blue: 0.349, alpha: 1), dark: UIColor(red: 0.188, green: 0.820, blue: 0.345, alpha: 1))
  static let tintAmber = dynamic(light: UIColor(red: 1.0, green: 0.584, blue: 0.0, alpha: 1), dark: UIColor(red: 1.0, green: 0.624, blue: 0.039, alpha: 1))

  static let undoReadyBackground = dynamic(
    light: UIColor(red: 1.0, green: 0.584, blue: 0.0, alpha: 0.15),
    dark: UIColor(red: 1.0, green: 0.624, blue: 0.039, alpha: 0.20)
  )

  static let undoReadyBorder = dynamic(
    light: UIColor(red: 1.0, green: 0.584, blue: 0.0, alpha: 0.32),
    dark: UIColor(red: 1.0, green: 0.624, blue: 0.039, alpha: 0.40)
  )

  static func kindColor(_ kind: KeyboardItemKind) -> UIColor {
    switch kind {
    case .otp: return tintOTP
    case .url: return tintURL
    case .image: return tintImage
    case .text: return tintText
    }
  }
}

struct KeyboardStrings {
  private let isChinese: Bool

  init(languages: [String] = Locale.preferredLanguages) {
    let first = languages.first?.lowercased() ?? "en"
    self.isChinese = first.hasPrefix("zh")
  }

  var railAll: String { isChinese ? "全部" : "All" }
  var railPinned: String { isChinese ? "置顶" : "Pinned" }
  var railLinks: String { isChinese ? "链接" : "Links" }
  var railImages: String { isChinese ? "图片" : "Images" }
  var railSync: String { isChinese ? "同步" : "Sync" }
  var railRetry: String { isChinese ? "重试" : "Retry" }

  var badgeLatest: String { isChinese ? "最新" : "Latest" }
  var badgeOTP: String { isChinese ? "验证码" : "Code" }
  var tapToCopyImage: String { isChinese ? "点按复制" : "Copy" }
  var copiedBadge: String { isChinese ? "已复制" : "Copied" }
  var imageCopyHint: String { isChinese ? "点按复制图片" : "Tap to copy image" }

  var undo: String { isChinese ? "撤销" : "Undo" }
  var space: String { isChinese ? "空格" : "space" }
  var spaceScrubbing: String { isChinese ? "← 移动光标 →" : "← Move Cursor →" }
  var deleteLabel: String { isChinese ? "退格删除" : "Delete" }
  var globeLabel: String { isChinese ? "切换键盘" : "Next Keyboard" }

  var pin: String { isChinese ? "置顶" : "Pin" }
  var unpin: String { isChinese ? "取消置顶" : "Unpin" }
  var pinnedTag: String { isChinese ? "已置顶" : "Pinned" }
  var insertIntoInput: String { isChinese ? "插入输入框" : "Insert" }
  var copyImage: String { isChinese ? "复制图片" : "Copy Image" }

  var toastInserted: String { isChinese ? "已插入" : "Inserted" }
  var toastOTPInserted: String { isChinese ? "已填入验证码" : "Code inserted" }
  var toastImageCopied: String { isChinese ? "图片已复制 · 点按输入框粘贴" : "Image copied · Tap field to paste" }
  var toastImageUnavailable: String { isChinese ? "图片未缓存，请在主 App 打开查看" : "Image unavailable" }
  var toastUndone: String { isChinese ? "已撤回上次插入" : "Undid last insert" }
  var toastNothingToUndo: String { isChinese ? "暂无可撤销内容" : "Nothing to undo" }
  var toastPinned: String { isChinese ? "已加入置顶" : "Pinned" }
  var toastUnpinned: String { isChinese ? "已取消置顶" : "Unpinned" }
  var toastSynced: String { isChinese ? "已刷新最新剪贴板" : "Clipboard refreshed" }
  var toastCapturedAndSynced: String { isChinese ? "已同步当前剪贴板到其他设备" : "Synced clipboard to devices" }
  var toastCapturedWaitingPeer: String { isChinese ? "已读取剪贴板 · 正在连接设备…" : "Clip saved · Connecting devices…" }
  var toastImageTooLarge: String { isChinese ? "图片过大，已跳过键盘自动同步" : "Image too large for keyboard sync" }
  var remoteDeviceFallback: String { isChinese ? "其他设备" : "Other device" }
  var sendClipImageTitle: String { isChinese ? "剪贴板图片" : "Clipboard Image" }
  var sendDetailClose: String { isChinese ? "收起" : "Close" }
  var sendRetrySingle: String { isChinese ? "重试" : "Retry" }

  func sendDetailHeader(clipTitle: String) -> String {
    let trimmed = clipTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
      return isChinese ? "发送明细" : "Send Details"
    }
    return isChinese ? "发送明细 · \(trimmed)" : "Send · \(trimmed)"
  }

  func sendPillHeadline(for batch: KeyboardSendBatchState) -> String {
    switch batch.overall {
    case .connecting:
      return isChinese ? "正在连接设备…" : "Connecting devices…"
    case .sending:
      if batch.totalCount == 1, let only = batch.devices.first {
        return isChinese ? "正在发送至 \(only.name)…" : "Sending to \(only.name)…"
      }
      return isChinese ? "正在同步 \(batch.deliveredCount)/\(batch.totalCount)" : "Syncing \(batch.deliveredCount)/\(batch.totalCount)"
    case .delivered:
      if batch.totalCount == 1, let only = batch.devices.first {
        return isChinese ? "已发送至 \(only.name)" : "Sent to \(only.name)"
      }
      return isChinese ? "已同步至 \(batch.totalCount) 台设备" : "Synced to \(batch.totalCount) devices"
    case .partial:
      if batch.deliveredCount == 0 && batch.failedCount == 0 {
        return isChinese ? "已存入本机 · 设备离线" : "Saved locally · Offline"
      }
      let issue: String
      if batch.failedCount > 0 {
        issue = isChinese ? "\(batch.failedCount) 台失败" : "\(batch.failedCount) failed"
      } else {
        issue = isChinese ? "\(batch.offlineCount) 台离线" : "\(batch.offlineCount) offline"
      }
      return isChinese ? "已同步 \(batch.deliveredCount)/\(batch.totalCount) · \(issue)" : "Synced \(batch.deliveredCount)/\(batch.totalCount) · \(issue)"
    }
  }

  func sendDeviceStateLabel(_ state: KeyboardSendDeviceState) -> String {
    switch state {
    case .waiting: return isChinese ? "建连中…" : "Connecting…"
    case .sending(let pct):
      if let pct, pct > 0 { return "\(pct)%" }
      return isChinese ? "发送中…" : "Sending…"
    case .delivered: return isChinese ? "已送达 ✓" : "Delivered ✓"
    case .offline: return isChinese ? "离线" : "Offline"
    case .failed: return isChinese ? "发送失败" : "Failed"
    }
  }

  func toastReceivedAndCopied(from deviceName: String, isImage: Bool) -> String {
    if isChinese {
      return isImage
        ? "已收到来自 \(deviceName) 的图片并复制到剪贴板"
        : "已收到来自 \(deviceName) 的内容并复制到剪贴板"
    }
    return isImage
      ? "Image from \(deviceName) copied"
      : "Clip from \(deviceName) copied"
  }

  var gateTitle: String { isChinese ? "开启「允许完全访问」以同步剪贴板" : "Allow Full Access to Sync Clips" }
  var gateMessage: String {
    isChinese
      ? "iOS 要求键盘扩展开启完全访问后，才能在打开期间读取与实时同步剪贴板内容。"
      : "iOS requires Full Access for the keyboard to read and sync your clipboard while active."
  }
  var gateChipNoKeylog: String { isChinese ? "不记录外部打字" : "No keystroke logging" }
  var gateChipLocalSync: String { isChinese ? "仅打开期间同步" : "Syncs only while open" }
  var gateOpenSettings: String { isChinese ? "前往系统设置 ›" : "Open Settings ›" }

  var emptyAllTitle: String { isChinese ? "暂无剪贴板记录" : "No Clipboard History" }
  var emptyPinnedTitle: String { isChinese ? "还没有置顶内容" : "No Pinned Clips" }
  var emptyLinksTitle: String { isChinese ? "暂无链接记录" : "No Links Yet" }
  var emptyImagesTitle: String { isChinese ? "暂无图片记录" : "No Images Yet" }
  var emptyAllMessage: String {
    isChinese
      ? "打开 Unic 或在其他设备复制文本、链接和图片后，会显示在这里。"
      : "Copy text, links, or images on your devices and open Unic to sync."
  }
  var emptyPinnedMessage: String {
    isChinese
      ? "长按任意剪贴卡片即可将其置顶，方便随时取用。"
      : "Long-press any clip to pin it for quick access."
  }

  var errorTitle: String { isChinese ? "暂未读取到剪贴快照" : "Snapshot Unavailable" }
  var errorMessage: String {
    isChinese
      ? "请先打开一次 Unic 主应用完成同步，然后点按重试。"
      : "Open Unic once to refresh your clipboard snapshot, then retry."
  }
  var retryButton: String { isChinese ? "重试刷新" : "Retry" }

  func returnTitle(for type: UIReturnKeyType) -> String {
    switch type {
    case .go: return isChinese ? "前往" : "Go"
    case .google, .search, .yahoo: return isChinese ? "搜索" : "Search"
    case .join: return isChinese ? "加入" : "Join"
    case .next: return isChinese ? "下一项" : "Next"
    case .route: return isChinese ? "路线" : "Route"
    case .send: return isChinese ? "发送" : "Send"
    case .done: return isChinese ? "完成" : "Done"
    case .continue: return isChinese ? "继续" : "Continue"
    default: return isChinese ? "换行" : "Return"
    }
  }

  func relativeTime(fromMs createdAtMs: Int64, nowMs: Int64 = Int64(Date().timeIntervalSince1970 * 1000)) -> String {
    let diffSec = max(0, (nowMs - createdAtMs) / 1000)
    if diffSec < 60 { return isChinese ? "刚刚" : "Now" }
    let mins = diffSec / 60
    if mins < 60 { return isChinese ? "\(mins)分钟前" : "\(mins)m" }
    let hours = mins / 60
    if hours < 24 { return isChinese ? "\(hours)小时前" : "\(hours)h" }
    let days = hours / 24
    return isChinese ? "\(days)天前" : "\(days)d"
  }
}
