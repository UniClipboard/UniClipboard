import { describe, expect, it } from 'vitest'
import { getChangePassphraseErrorCode } from '@/api/daemon/encryption'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'

describe('passphrase change errors', () => {
  it('reads the semantic code from a normalized daemon error', () => {
    const error = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on endpoint', {
      code: 'MULTIPLE_DEVICES',
      message: 'redacted public message',
    })

    expect(getChangePassphraseErrorCode(error)).toBe('MULTIPLE_DEVICES')
  })

  it('rejects unknown and malformed error details', () => {
    expect(
      getChangePassphraseErrorCode(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'failed', { code: 'NEW_CODE' })
      )
    ).toBeNull()
    expect(getChangePassphraseErrorCode(new Error('failed'))).toBeNull()
  })
})
