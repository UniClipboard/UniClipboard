/**
 * MobileSyncSettingsDialog —— helper 单测。
 *
 * 只覆盖 translateMobileSyncError —— Dialog 自身渲染需要 mock Tauri command +
 * AlertDialog + portal 等基础设施,ROI 太低。helper 是 settings 路径错误翻译
 * 的真相源(7 个 variant),值得锁住。
 */

import '@testing-library/jest-dom/vitest'
import { beforeAll, describe, expect, it } from 'vitest'
import { __test__ } from '@/components/device/MobileSyncSettingsDialog'
import i18n from '@/i18n'

const { translateMobileSyncError } = __test__

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

const t = i18n.t.bind(i18n)

describe('MobileSyncSettingsDialog.translateMobileSyncError — settings 路径变体', () => {
  it('FACADE_UNAVAILABLE → 功能未启用', () => {
    expect(translateMobileSyncError(t, { code: 'FACADE_UNAVAILABLE' })).toContain('未启用')
  })

  it('INVALID_LAN_PARAMETER → 含 reason', () => {
    const result = translateMobileSyncError(t, {
      code: 'INVALID_LAN_PARAMETER',
      reason: 'lan_port=0',
    })
    expect(result).toContain('lan_port=0')
  })

  it('SETTINGS_LOAD_FAILED → 含 message + 加载', () => {
    const result = translateMobileSyncError(t, {
      code: 'SETTINGS_LOAD_FAILED',
      message: 'disk full',
    })
    expect(result).toContain('disk full')
    expect(result).toContain('加载')
  })

  it('SETTINGS_SAVE_FAILED → 含 message + 保存', () => {
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

  it('register 路径专属 variant(USERNAME_TAKEN)→ 兜底 unknown', () => {
    const result = translateMobileSyncError(t, {
      code: 'USERNAME_TAKEN',
      username: 'mobile_alice',
    })
    expect(result).toContain('USERNAME_TAKEN')
  })

  it('未知 code(Error 实例)→ 兜底 unknown 含 message', () => {
    const result = translateMobileSyncError(t, new Error('boom'))
    expect(result).toContain('boom')
  })

  it('字符串错误 → 兜底 unknown 用 String(err)', () => {
    expect(translateMobileSyncError(t, 'something bad')).toContain('something bad')
  })
})
