import { describe, expect, it } from 'vitest'
import { applyPlatformEffectPreferences, detectPlatformInfo } from '@/lib/platform'

describe('platform helpers', () => {
  it('识别 Linux 平台', () => {
    const platform = detectPlatformInfo({
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15',
      platform: 'Linux x86_64',
      isTauri: true,
    })

    expect(platform.isLinux).toBe(true)
  })

  it('识别 Windows 平台', () => {
    const platform = detectPlatformInfo({
      userAgent:
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Edg/120.0',
      platform: 'Win32',
      isTauri: true,
    })

    expect(platform.isWindows).toBe(true)
  })

  it('不会把 Android 当作桌面 Linux', () => {
    const platform = detectPlatformInfo({
      userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36',
      platform: 'Linux armv8l',
      isTauri: false,
    })

    expect(platform.isLinux).toBe(false)
  })

  it('只写入平台标记，不再决定视觉效果', () => {
    const root = document.createElement('html')

    applyPlatformEffectPreferences(root, {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
    })

    expect(root.dataset.ucPlatform).toBe('linux')
    expect(root.dataset.ucLowEffects).toBeUndefined()
  })
})
