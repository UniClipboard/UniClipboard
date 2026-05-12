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
