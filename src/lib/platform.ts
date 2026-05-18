export interface PlatformInfo {
  isWindows: boolean
  isMac: boolean
  isLinux: boolean
  isTauri: boolean
  reduceVisualEffects: boolean
}

interface PlatformProbe {
  userAgent?: string
  platform?: string
  tauriPlatform?: string
  isTauri?: boolean
}

const normalize = (value?: string): string => value?.toLowerCase() ?? ''

const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' &&
  Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)

const readPlatformProbe = (): PlatformProbe => {
  const tauriWindow =
    typeof window === 'undefined'
      ? undefined
      : (window as unknown as { __TAURI__?: { platform?: string } })
  const nav =
    typeof navigator === 'undefined'
      ? undefined
      : (navigator as Navigator & { userAgentData?: { platform?: string } })

  return {
    userAgent: nav?.userAgent,
    platform: nav?.userAgentData?.platform ?? nav?.platform,
    tauriPlatform: tauriWindow?.__TAURI__?.platform,
    isTauri: isTauriEnv(),
  }
}

export const detectPlatformInfo = (probe: PlatformProbe = readPlatformProbe()): PlatformInfo => {
  const userAgent = normalize(probe.userAgent)
  const platform = normalize(probe.platform)
  const tauriPlatform = normalize(probe.tauriPlatform)
  const isAndroid = userAgent.includes('android')
  const isWindows =
    userAgent.includes('windows') || platform.includes('win') || tauriPlatform === 'windows'
  const isMac =
    userAgent.includes('macintosh') ||
    userAgent.includes('mac os') ||
    platform.includes('mac') ||
    tauriPlatform === 'macos'
  const isLinux =
    !isAndroid &&
    (userAgent.includes('linux') ||
      platform.includes('linux') ||
      platform.includes('x11') ||
      tauriPlatform === 'linux')

  return {
    isWindows,
    isMac,
    isLinux,
    isTauri: probe.isTauri ?? false,
    reduceVisualEffects: isLinux,
  }
}

export const applyPlatformEffectPreferences = (
  root: HTMLElement | null = typeof document === 'undefined' ? null : document.documentElement,
  platform: PlatformInfo = detectPlatformInfo()
): void => {
  if (!root) {
    return
  }

  root.dataset.ucPlatform = platform.isLinux
    ? 'linux'
    : platform.isWindows
      ? 'windows'
      : platform.isMac
        ? 'macos'
        : 'unknown'
  root.dataset.ucLowEffects = platform.reduceVisualEffects ? 'true' : 'false'
}

export const isLowEffectsEnabled = (): boolean =>
  typeof document !== 'undefined' && document.documentElement.dataset.ucLowEffects === 'true'

/**
 * Windows 主窗口 DWM 材质状态 —— Rust 侧实际装配结果，由 webview 启动后
 * 通过 `commands.getMainWindowMaterial()` 异步取回，再 patch 到 `<html>`
 * 的 `data-uc-window-material` attribute 上。`globals.css` 据此决定
 * `--background` token 走透明还是 opaque：
 *
 *   `[data-uc-platform="windows"]` 默认 opaque（与现状一致）；
 *   `[data-uc-platform="windows"][data-uc-window-material="mica"]` 切透明。
 *
 * 这样 Win 10 / 早期 Win 11（Mica 装不上）不会出现"窗口先透明再变白"的
 * 反向闪烁；Win 11 22H2+ 会在装配完成后从 opaque 切到透明（短暂闪烁，
 * 方向是"看见内容 → 看见 Mica"，肉眼无感）。
 *
 * 非 Windows 平台 / material 为 'none' 时不设 attr（或显式设 'none'），
 * 不影响 macOS hudWindow / Linux opaque 的现有行为。
 */
export const applyWindowMaterial = (
  material: 'mica' | 'none',
  root: HTMLElement | null = typeof document === 'undefined' ? null : document.documentElement
): void => {
  if (!root) {
    return
  }
  root.dataset.ucWindowMaterial = material
}
