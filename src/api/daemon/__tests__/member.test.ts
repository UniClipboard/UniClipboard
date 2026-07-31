import { describe, expect, it } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { isLegacyBootstrapRequired } from '@/api/daemon/member'

describe('isLegacyBootstrapRequired', () => {
  it('recognizes the normalized daemon conflict body', () => {
    const error = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /pairing/unpair', {
      code: 'legacy_bootstrap_required',
      message: 'legacy Space member removal requires secure bootstrap',
    })

    expect(isLegacyBootstrapRequired(error)).toBe(true)
  })

  it('rejects unrelated and nested conflict codes', () => {
    const unrelated = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /pairing/unpair', {
      code: 'conflict',
    })
    const nested = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /pairing/unpair', {
      details: { code: 'legacy_bootstrap_required' },
    })

    expect(isLegacyBootstrapRequired(unrelated)).toBe(false)
    expect(isLegacyBootstrapRequired(nested)).toBe(false)
    expect(isLegacyBootstrapRequired(new Error('legacy_bootstrap_required'))).toBe(false)
  })
})
