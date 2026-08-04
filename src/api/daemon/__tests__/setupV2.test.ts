import { afterEach, describe, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { redeemInvitation, switchSpace } from '@/api/daemon/setupV2'

function sponsorUpgradeRequiredError(path: string): DaemonApiError {
  return new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, `409 on ${path}`, {
    code: 'sponsor_upgrade_required',
    message: 'host version is incompatible',
  })
}

function unreadableHistoryConfirmationError(path: string): DaemonApiError {
  return new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, `409 on ${path}`, {
    code: 'unreadable_history_confirmation_required',
    message: 'explicit confirmation is required',
  })
}

describe('setup v2 sponsor upgrade errors', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('classifies a fresh join by the stable daemon error code', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      sponsorUpgradeRequiredError('/v2/setup/redeem')
    )

    await expect(
      redeemInvitation({ code: 'ABCD1234', passphrase: 'secret' })
    ).rejects.toMatchObject({ kind: 'sponsor_upgrade_required' })
  })

  it('classifies a space switch by the stable daemon error code', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      sponsorUpgradeRequiredError('/v2/setup/switch-space')
    )

    await expect(switchSpace({ code: 'ABCD1234', newPassphrase: 'secret' })).rejects.toMatchObject({
      kind: 'sponsor_upgrade_required',
    })
  })

  it('keeps unreadable-history confirmation distinct from a generic switch failure', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      unreadableHistoryConfirmationError('/v2/setup/switch-space')
    )

    await expect(switchSpace({ code: 'ABCD1234', newPassphrase: 'secret' })).rejects.toMatchObject({
      kind: 'unreadable_history_confirmation_required',
    })
  })
})
