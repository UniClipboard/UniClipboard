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
 * 主窗口当前实际生效的系统材质，与后端 `MainWindowMaterial` enum 同源。
 * 不直接 import generated type 是为了避免 `platform.ts` 反向依赖 `@/lib/ipc`，
 * 这里的 union 由 specta_export 测试保证两端 wire format 一致。
 */
export type WindowMaterial = 'mica' | 'hud' | 'none'

/**
 * 主窗口系统材质状态 —— Rust 侧通过 `commands.getMainWindowMaterial()` 异步
 * 报告，写到 `<html>` 的 `data-uc-window-material`。`globals.css` 据此决定
 * `--background`/`--sidebar`/`--card`/`--popover` 走透明还是 opaque：
 *
 *   `"mica"` (Win 11 22H2+) / `"hud"` (macOS NSVisualEffectView) → 透明，
 *   让底层材质透出来；
 *   `"none"` (Linux / Win 10 / 早期 Win 11 / 装配失败) → opaque 兜底。
 *
 * 启动时先走 opaque（CSS 默认），invoke 拿到结果再 patch attribute —— Mica
 * 装不上的情况下不会出现"先透明再变白"的反向闪烁；Mica/Hud 命中时是
 * "看见内容 → 看见材质"的正向过渡，肉眼几乎无感。
 */
export const applyWindowMaterial = (
  material: WindowMaterial,
  root: HTMLElement | null = typeof document === 'undefined' ? null : document.documentElement
): void => {
  if (!root) {
    return
  }
  root.dataset.ucWindowMaterial = material
}
