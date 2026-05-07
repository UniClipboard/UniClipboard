/**
 * MobileShortcutDevicesPanel —— helper 单测。
 *
 * 这里只覆盖 `translateMobileSyncError` 与 `formatRelativeMs` 两个纯函数;
 * Panel 自身的渲染测试需要 mock 6 个 Tauri command + i18n + AlertDialog 框架,
 * ROI 太低 —— 端到端走 Step 8 的真机回归更划算。helper 是错误翻译的真相
 * 源,17 个 MobileSyncError variant 的每条 i18n 路径都从这里走,值得锁住。
 */

import '@testing-library/jest-dom/vitest'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { __test__ } from '@/components/device/MobileShortcutDevicesPanel'
import i18n from '@/i18n'

const { translateMobileSyncError, formatRelativeMs } = __test__

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

const t = (key: string, options?: Record<string, unknown>) => i18n.t(key, options)

describe('translateMobileSyncError — 17 个 variant 全覆盖', () => {
  it('FACADE_UNAVAILABLE → 功能未启用', () => {
    expect(translateMobileSyncError(t, { code: 'FACADE_UNAVAILABLE' })).toContain('未启用')
  })

  it('LABEL_EMPTY → 名称不能为空', () => {
    expect(translateMobileSyncError(t, { code: 'LABEL_EMPTY' })).toContain('不能为空')
  })

  it('LABEL_TOO_LONG → 含 max', () => {
    const result = translateMobileSyncError(t, { code: 'LABEL_TOO_LONG', max: 64 })
    expect(result).toContain('64')
    expect(result).toContain('过长')
  })

  it('LAN_LISTENER_DISABLED → 提示先启用', () => {
    expect(translateMobileSyncError(t, { code: 'LAN_LISTENER_DISABLED' })).toContain('LAN')
  })

  it('USERNAME_TAKEN → 含 username', () => {
    const result = translateMobileSyncError(t, {
      code: 'USERNAME_TAKEN',
      username: 'mobile_alice',
    })
    expect(result).toContain('mobile_alice')
    expect(result).toContain('占用')
  })

  it('USERNAME_INVALID_SHAPE → 含 reason', () => {
    const result = translateMobileSyncError(t, {
      code: 'USERNAME_INVALID_SHAPE',
      reason: 'must start with a letter',
    })
    expect(result).toContain('must start with a letter')
  })

  it('PASSWORD_TOO_SHORT → 含 min 数字', () => {
    const result = translateMobileSyncError(t, { code: 'PASSWORD_TOO_SHORT', min: 8 })
    expect(result).toContain('8')
    expect(result).toContain('不足')
  })

  it('PASSWORD_TOO_LONG → 含 max 数字', () => {
    const result = translateMobileSyncError(t, { code: 'PASSWORD_TOO_LONG', max: 256 })
    expect(result).toContain('256')
  })

  it('PASSWORD_HASH_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'PASSWORD_HASH_FAILED',
      message: 'argon2 oom',
    })
    expect(result).toContain('argon2 oom')
  })

  it('DEVICE_NOT_FOUND → 提示刷新', () => {
    const result = translateMobileSyncError(t, {
      code: 'DEVICE_NOT_FOUND',
      deviceId: 'did_xxx',
    })
    expect(result).toContain('刷新')
  })

  it('INVALID_LAN_PARAMETER → 含 reason', () => {
    const result = translateMobileSyncError(t, {
      code: 'INVALID_LAN_PARAMETER',
      reason: 'lan_port=0',
    })
    expect(result).toContain('lan_port=0')
  })

  it('SETTINGS_LOAD_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'SETTINGS_LOAD_FAILED',
      message: 'disk full',
    })
    expect(result).toContain('disk full')
    expect(result).toContain('加载')
  })

  it('SETTINGS_SAVE_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'SETTINGS_SAVE_FAILED',
      message: 'permission denied',
    })
    expect(result).toContain('permission denied')
    expect(result).toContain('保存')
  })

  it('ENDPOINT_INFO_PROBE_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'ENDPOINT_INFO_PROBE_FAILED',
      message: 'no iface',
    })
    expect(result).toContain('no iface')
  })

  it('LAN_PROBE_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'LAN_PROBE_FAILED',
      message: 'EACCES',
    })
    expect(result).toContain('EACCES')
  })

  it('PERSISTENCE_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'PERSISTENCE_FAILED',
      message: 'sqlite locked',
    })
    expect(result).toContain('sqlite locked')
  })

  it('QR_RENDER_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'QR_RENDER_FAILED',
      message: 'url too long',
    })
    expect(result).toContain('url too long')
  })

  it('未知 code(非 MobileSyncError 形态)→ 兜底 unknown', () => {
    // 没有 code 字段的 raw Error
    const err = new Error('boom')
    const result = translateMobileSyncError(t, err)
    expect(result).toContain('boom')
  })

  it('字符串错误 → 兜底 unknown 用 String(err)', () => {
    const result = translateMobileSyncError(t, 'something bad')
    expect(result).toContain('something bad')
  })
})

describe('formatRelativeMs', () => {
  it('< 1 分钟 → just now（20s 远低于 round 边界）', () => {
    // 注意:helper 用 Math.round,30_000ms 会进位成 1m;用 20s 安全落在 0
    expect(formatRelativeMs(Date.now() - 20_000)).toBe('just now')
  })

  it('5 分钟前 → 5m', () => {
    expect(formatRelativeMs(Date.now() - 5 * 60_000)).toBe('5m')
  })

  it('2 小时前 → 2h', () => {
    expect(formatRelativeMs(Date.now() - 2 * 60 * 60_000)).toBe('2h')
  })

  it('3 天前 → 3d', () => {
    expect(formatRelativeMs(Date.now() - 3 * 24 * 60 * 60_000)).toBe('3d')
  })

  it('刚好 60 分钟 → 1h(分钟段下边界)', () => {
    // 60 分钟 = 边界,期望落到 60/60 = 1h 段
    vi.useFakeTimers()
    const fixedNow = 1_000_000_000_000
    vi.setSystemTime(fixedNow)
    expect(formatRelativeMs(fixedNow - 60 * 60_000)).toBe('1h')
    vi.useRealTimers()
  })
})
