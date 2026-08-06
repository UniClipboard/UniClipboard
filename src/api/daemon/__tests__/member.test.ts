import { describe, expect, it } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { isLegacyBootstrapRequired, isMemberRemovalBlocked } from '@/api/daemon/member'

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

describe('isMemberRemovalBlocked', () => {
  it.each(['member_removal_in_progress', 'member_removal_recovery_required'])(
    'recognizes the stable conflict %s returned while another removal blocks cancellation',
    code => {
      const error = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /pairing/unpair', {
        code,
      })

      expect(isMemberRemovalBlocked(error)).toBe(true)
    }
  )

  it('rejects unrelated errors', () => {
    const unrelated = new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, '409 on /pairing/unpair', {
      code: 'legacy_bootstrap_required',
    })

    expect(isMemberRemovalBlocked(unrelated)).toBe(false)
    expect(isMemberRemovalBlocked(new Error('member_removal_in_progress'))).toBe(false)
  })
})
