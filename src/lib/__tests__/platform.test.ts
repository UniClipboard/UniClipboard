import { describe, expect, it } from 'vitest'
import { applyPlatformEffectPreferences, detectPlatformInfo } from '@/lib/platform'

describe('platform helpers', () => {
  it('为 Linux 开启低特效模式', () => {
    const platform = detectPlatformInfo({
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15',
      platform: 'Linux x86_64',
      isTauri: true,
    })

    expect(platform.isLinux).toBe(true)
    expect(platform.reduceVisualEffects).toBe(true)
  })

  it('不会把 Android 当作桌面 Linux', () => {
    const platform = detectPlatformInfo({
      userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36',
      platform: 'Linux armv8l',
      isTauri: false,
    })

    expect(platform.isLinux).toBe(false)
    expect(platform.reduceVisualEffects).toBe(false)
  })

  it('把低特效标记写到根节点', () => {
    const root = document.createElement('html')

    applyPlatformEffectPreferences(root, {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
      reduceVisualEffects: true,
    })

    expect(root.dataset.ucPlatform).toBe('linux')
    expect(root.dataset.ucLowEffects).toBe('true')
  })
})
