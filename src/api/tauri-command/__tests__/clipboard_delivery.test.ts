import { describe, expect, it } from 'vitest'
import {
  isResendEntryError,
  type ResendEntryCommandError,
} from '@/api/tauri-command/clipboard_delivery'

describe('isResendEntryError — typed error guard', () => {
  // 已知 wire-format `code` 全部要被守卫接住。Rust 端
  // `ResendEntryCommandError` 的 `#[serde(tag = "code", rename_all =
  // "SCREAMING_SNAKE_CASE")]` 变体名变更时,这里要一起改 —— 守卫的白名单
  // 是该契约的前端镜像。
  it.each([
    { code: 'ENTRY_NOT_FOUND', entryId: 'ent-1' },
    { code: 'ENTRY_NOT_RESENDABLE', entryId: 'ent-2', reason: 'remoteOrigin' },
    { code: 'ENTRY_NOT_RESENDABLE', entryId: 'ent-3', reason: 'payloadLost' },
    { code: 'TARGET_NOT_TRUSTED', deviceId: 'dev-x' },
    { code: 'NO_ELIGIBLE_TARGETS' },
    { code: 'STORAGE', message: 'db down' },
    { code: 'DISPATCH', message: 'encrypt session locked' },
  ] satisfies ReadonlyArray<ResendEntryCommandError>)(
    'recognises wire envelope with code=$code',
    payload => {
      expect(isResendEntryError(payload)).toBe(true)
    }
  )

  // 其它 typed command (例如 mobile_sync) 也走 `{ code: string, ... }`
  // 形态。若守卫只看 `typeof code === 'string'`,这些 envelope 会被误识别成
  // resend 错误,触发 `switch (e.code)` 的 fallback 分支(toast 显示
  // `internal` 而不是上游真正的语义)。
  it('rejects unknown code values to avoid cross-command confusion', () => {
    expect(isResendEntryError({ code: 'MOBILE_SYNC_FAILED', message: 'x' })).toBe(false)
    expect(isResendEntryError({ code: 'entry_not_found' })).toBe(false) // case sensitive
    expect(isResendEntryError({ code: '' })).toBe(false)
  })

  it('rejects shapes without a string code', () => {
    expect(isResendEntryError(null)).toBe(false)
    expect(isResendEntryError(undefined)).toBe(false)
    expect(isResendEntryError(new Error('boom'))).toBe(false)
    expect(isResendEntryError({ message: 'no code field' })).toBe(false)
    expect(isResendEntryError({ code: 123 })).toBe(false)
    expect(isResendEntryError('ENTRY_NOT_FOUND')).toBe(false)
  })
})
