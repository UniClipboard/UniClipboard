export interface PlatformInfo {
  isWindows: boolean
  isMac: boolean
  isLinux: boolean
  isDesktopHost: boolean
}

interface PlatformProbe {
  userAgent?: string
  platform?: string
  isDesktopHost?: boolean
}

const normalize = (value?: string): string => value?.toLowerCase() ?? ''

const isDesktopHostEnv = (): boolean =>
  typeof window !== 'undefined' &&
  Boolean((window as unknown as { __UC_DESKTOP_HOST__?: unknown }).__UC_DESKTOP_HOST__)

const readPlatformProbe = (): PlatformProbe => {
  const nav =
    typeof navigator === 'undefined'
      ? undefined
      : (navigator as Navigator & { userAgentData?: { platform?: string } })

  return {
    userAgent: nav?.userAgent,
    platform: nav?.userAgentData?.platform ?? nav?.platform,
    isDesktopHost: isDesktopHostEnv(),
  }
}

export const detectPlatformInfo = (probe: PlatformProbe = readPlatformProbe()): PlatformInfo => {
  const userAgent = normalize(probe.userAgent)
  const platform = normalize(probe.platform)
  const isAndroid = userAgent.includes('android')
  const isWindows = userAgent.includes('windows') || platform.includes('win')
  const isMac =
    userAgent.includes('macintosh') || userAgent.includes('mac os') || platform.includes('mac')
  const isLinux =
    !isAndroid &&
    (userAgent.includes('linux') || platform.includes('linux') || platform.includes('x11'))

  return {
    isWindows,
    isMac,
    isLinux,
    isDesktopHost: probe.isDesktopHost ?? false,
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
}
