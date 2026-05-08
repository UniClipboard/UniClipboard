/**
 * MobileSyncDevicesPanel —— helper 单测(方案 B 重构后)。
 *
 * 方案 B 把 settings 类错误翻译搬到了 MobileSyncSettingsSheet,所以
 * panel 自身的 translateMobileSyncError 只覆盖 list/revoke 路径会触发的
 * variant + 兜底。其余变体走 default → unknown bucket。
 */

import '@testing-library/jest-dom/vitest'
import { beforeAll, describe, expect, it } from 'vitest'
import { __test__ } from '@/components/device/MobileSyncDevicesPanel'
import i18n from '@/i18n'

const { translateMobileSyncError } = __test__

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

const t = i18n.t.bind(i18n)

describe('Panel.translateMobileSyncError — list/revoke 路径变体', () => {
  it('FACADE_UNAVAILABLE → 功能未启用', () => {
    expect(translateMobileSyncError(t, { code: 'FACADE_UNAVAILABLE' })).toContain('未启用')
  })

  it('LAN_LISTENER_DISABLED → 提示先启用', () => {
    expect(translateMobileSyncError(t, { code: 'LAN_LISTENER_DISABLED' })).toContain('LAN')
  })

  it('DEVICE_NOT_FOUND → 提示刷新', () => {
    const result = translateMobileSyncError(t, {
      code: 'DEVICE_NOT_FOUND',
      deviceId: 'did_xxx',
    })
    expect(result).toContain('刷新')
  })

  it('PERSISTENCE_FAILED → 含 message', () => {
    const result = translateMobileSyncError(t, {
      code: 'PERSISTENCE_FAILED',
      message: 'sqlite locked',
    })
    expect(result).toContain('sqlite locked')
  })

  it('SETTINGS_LOAD_FAILED → 含 message(初次 list 失败可能落到这里)', () => {
    const result = translateMobileSyncError(t, {
      code: 'SETTINGS_LOAD_FAILED',
      message: 'disk full',
    })
    expect(result).toContain('disk full')
    expect(result).toContain('加载')
  })
})

describe('Panel.translateMobileSyncError — 其它 variant 走兜底 unknown', () => {
  it('SETTINGS_SAVE_FAILED → unknown(已搬到 SettingsSheet)', () => {
    const result = translateMobileSyncError(t, {
      code: 'SETTINGS_SAVE_FAILED',
      message: 'permission denied',
    })
    expect(result).toContain('permission denied')
  })

  it('USERNAME_TAKEN → unknown(register 路径不归本组件)', () => {
    const result = translateMobileSyncError(t, {
      code: 'USERNAME_TAKEN',
      username: 'mobile_alice',
    })
    // 兜底 unknown 取 message ?? code,USERNAME_TAKEN 没有 message,落 code
    expect(result).toContain('USERNAME_TAKEN')
  })

  it('未知 code(非 MobileSyncError 形态)→ 兜底 unknown', () => {
    const err = new Error('boom')
    const result = translateMobileSyncError(t, err)
    expect(result).toContain('boom')
  })

  it('字符串错误 → 兜底 unknown 用 String(err)', () => {
    const result = translateMobileSyncError(t, 'something bad')
    expect(result).toContain('something bad')
  })
})
