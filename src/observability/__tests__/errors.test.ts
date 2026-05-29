import { describe, expect, it } from 'vitest'
import { isExpectedCommandError, toReportableError } from '../errors'

describe('toReportableError', () => {
  it('passes Error instances through unchanged', () => {
    const err = new Error('boom')
    expect(toReportableError(err, 'foo')).toBe(err)
  })

  it('passes non-object rejections through unchanged', () => {
    expect(toReportableError('weird', 'foo')).toBe('weird')
    expect(toReportableError(null, 'foo')).toBe(null)
    expect(toReportableError(42, 'foo')).toBe(42)
  })

  it('wraps typed-error envelopes into Error with command × code in the message', () => {
    const envelope = { code: 'USERNAME_TAKEN', username: 'alice' }
    const result = toReportableError(envelope, 'registerMobileDevice')

    expect(result).toBeInstanceOf(Error)
    const wrapped = result as Error & { cause?: unknown }
    expect(wrapped.message).toBe('Tauri command "registerMobileDevice" failed: USERNAME_TAKEN')
    expect(wrapped.name).toBe('TauriCommandError(USERNAME_TAKEN)')
    expect(wrapped.cause).toEqual({ code: 'USERNAME_TAKEN', username: 'alice' })
  })

  it('redacts sensitive fields on cause', () => {
    const envelope = { code: 'PERSISTENCE_FAILED', password: 'hunter2', nested: { token: 'abc' } }
    const result = toReportableError(envelope, 'rotateMobilePassword') as Error & {
      cause?: Record<string, unknown>
    }

    expect(result.cause).toEqual({
      code: 'PERSISTENCE_FAILED',
      password: '[REDACTED]',
      nested: { token: '[REDACTED]' },
    })
  })

  it('leaves objects without a code field alone', () => {
    const weird = { foo: 'bar' }
    expect(toReportableError(weird, 'cmd')).toBe(weird)
  })
})

describe('isExpectedCommandError', () => {
  it('recognizes user/validation error codes as expected', () => {
    expect(isExpectedCommandError({ code: 'USERNAME_MUST_START_WITH_LETTER' })).toBe(true)
    expect(isExpectedCommandError({ code: 'WRONG_PASSPHRASE' })).toBe(true)
    expect(isExpectedCommandError({ code: 'USERNAME_TAKEN', username: 'alice' })).toBe(true)
  })

  it('treats system error codes as unexpected (reportable)', () => {
    expect(isExpectedCommandError({ code: 'PERSISTENCE_FAILED', message: 'x' })).toBe(false)
    expect(isExpectedCommandError({ code: 'INTERNAL', message: 'x' })).toBe(false)
    expect(isExpectedCommandError({ code: 'PASSWORD_HASH_FAILED', message: 'x' })).toBe(false)
  })

  it('treats unknown codes and non-envelopes as unexpected (fail-safe)', () => {
    expect(isExpectedCommandError({ code: 'SOME_FUTURE_CODE' })).toBe(false)
    expect(isExpectedCommandError(new Error('boom'))).toBe(false)
    expect(isExpectedCommandError('weird')).toBe(false)
    expect(isExpectedCommandError(null)).toBe(false)
    expect(isExpectedCommandError({ code: 42 })).toBe(false)
  })
})
