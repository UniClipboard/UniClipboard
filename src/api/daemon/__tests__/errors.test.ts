import { describe, expect, it } from 'vitest'
import { DaemonApiError, DaemonErrorCode, mapStatusToErrorCode } from '@/api/daemon/errors'

describe('DaemonErrorCode', () => {
  it('contains all expected error codes', () => {
    expect(DaemonErrorCode.UNAUTHORIZED).toBe('UNAUTHORIZED')
    expect(DaemonErrorCode.FORBIDDEN).toBe('FORBIDDEN')
    expect(DaemonErrorCode.NOT_FOUND).toBe('NOT_FOUND')
    expect(DaemonErrorCode.RATE_LIMITED).toBe('RATE_LIMITED')
    expect(DaemonErrorCode.ENCRYPTION_NOT_READY).toBe('ENCRYPTION_NOT_READY')
    expect(DaemonErrorCode.CONFIRMATION_REQUIRED).toBe('CONFIRMATION_REQUIRED')
    expect(DaemonErrorCode.INTERNAL_ERROR).toBe('INTERNAL_ERROR')
  })
})

describe('DaemonApiError', () => {
  it('extends Error and sets name to DaemonApiError', () => {
    const err = new DaemonApiError(DaemonErrorCode.UNAUTHORIZED, 'bad token')
    expect(err).toBeInstanceOf(Error)
    expect(err).toBeInstanceOf(DaemonApiError)
    expect(err.name).toBe('DaemonApiError')
  })

  it('populates code and message from constructor args', () => {
    const err = new DaemonApiError(DaemonErrorCode.FORBIDDEN, 'access denied')
    expect(err.code).toBe(DaemonErrorCode.FORBIDDEN)
    expect(err.message).toBe('access denied')
  })

  it('stores details when provided', () => {
    const details = { field: 'passphrase', reason: 'too short' }
    const err = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'validation failed', details)
    expect(err.details).toEqual(details)
  })

  it('leaves details undefined when omitted', () => {
    const err = new DaemonApiError(DaemonErrorCode.NOT_FOUND, 'not found')
    expect(err.details).toBeUndefined()
  })

  it('has a usable stack trace', () => {
    const err = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'boom')
    expect(err.stack).toBeDefined()
    expect(err.stack).toContain('DaemonApiError')
  })
})

describe('mapStatusToErrorCode', () => {
  const cases: Array<[number, DaemonErrorCode]> = [
    [401, DaemonErrorCode.UNAUTHORIZED],
    [403, DaemonErrorCode.FORBIDDEN],
    [404, DaemonErrorCode.NOT_FOUND],
    [429, DaemonErrorCode.RATE_LIMITED],
    [503, DaemonErrorCode.ENCRYPTION_NOT_READY],
  ]

  it.each(cases)('maps HTTP %i → %s', (status, expectedCode) => {
    expect(mapStatusToErrorCode(status)).toBe(expectedCode)
  })

  it('maps unknown status codes to INTERNAL_ERROR', () => {
    expect(mapStatusToErrorCode(500)).toBe(DaemonErrorCode.INTERNAL_ERROR)
    expect(mapStatusToErrorCode(502)).toBe(DaemonErrorCode.INTERNAL_ERROR)
    expect(mapStatusToErrorCode(418)).toBe(DaemonErrorCode.INTERNAL_ERROR)
    expect(mapStatusToErrorCode(0)).toBe(DaemonErrorCode.INTERNAL_ERROR)
  })
})
