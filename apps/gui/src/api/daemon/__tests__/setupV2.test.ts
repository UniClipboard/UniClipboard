import { afterEach, describe, expect, it, vi } from 'vitest'
import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { issuePairingInvitation, redeemInvitation, switchSpace } from '@/api/daemon/setupV2'
import * as sdk from '@/api/generated/sdk.gen'

describe('six-digit invitation requests', () => {
  afterEach(() => vi.restoreAllMocks())

  it.each(['000001', '000-001'])('submits the complete canonical code for %s', async code => {
    vi.spyOn(daemonClient, 'callEnveloped').mockImplementation(async call => {
      await call()
      return {} as never
    })
    const redeem = vi.spyOn(sdk, 'setupV2Redeem').mockResolvedValue({} as never)
    const switchRequest = vi.spyOn(sdk, 'setupV2SwitchSpace').mockResolvedValue({} as never)

    await redeemInvitation({ code, passphrase: 'secret' })
    await switchSpace({ code, newPassphrase: 'secret' })

    expect(redeem).toHaveBeenCalledWith(
      expect.objectContaining({
        body: expect.objectContaining({ code: '000-001' }),
      })
    )
    expect(switchRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        body: expect.objectContaining({ code: '000-001' }),
      })
    )
  })
})

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

function invitationIssueError(code: string, status: number): DaemonApiError {
  return new DaemonApiError(
    DaemonErrorCode.INTERNAL_ERROR,
    `${status} on /v2/setup/issue-invitation`,
    {
      code,
      message: 'safe invitation failure',
    }
  )
}

describe('setup v2 invitation issue errors', () => {
  afterEach(() => vi.restoreAllMocks())

  it.each([
    ['invitation_no_publishable_address', 503, 'no_publishable_address'],
    ['invitation_local_publication_failed', 503, 'local_publication_failed'],
    ['invitation_directory_transport_failed', 503, 'directory_transport_failed'],
    ['invitation_directory_rejected', 409, 'directory_rejected'],
    ['invitation_directory_invalid_response', 503, 'directory_invalid_response'],
  ])('classifies %s by its stable daemon code', async (code, status, kind) => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(invitationIssueError(code, status))

    await expect(issuePairingInvitation()).rejects.toMatchObject({
      kind,
      raw: 'safe invitation failure',
      httpStatus: status,
    })
  })
})

describe('setup v2 sponsor upgrade errors', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('classifies a fresh join by the stable daemon error code', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      sponsorUpgradeRequiredError('/v2/setup/redeem')
    )

    await expect(redeemInvitation({ code: '012345', passphrase: 'secret' })).rejects.toMatchObject({
      kind: 'sponsor_upgrade_required',
    })
  })

  it('classifies a space switch by the stable daemon error code', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      sponsorUpgradeRequiredError('/v2/setup/switch-space')
    )

    await expect(switchSpace({ code: '012345', newPassphrase: 'secret' })).rejects.toMatchObject({
      kind: 'sponsor_upgrade_required',
    })
  })

  it('keeps unreadable-history confirmation distinct from a generic switch failure', async () => {
    vi.spyOn(daemonClient, 'callEnveloped').mockRejectedValue(
      unreadableHistoryConfirmationError('/v2/setup/switch-space')
    )

    await expect(switchSpace({ code: '012345', newPassphrase: 'secret' })).rejects.toMatchObject({
      kind: 'unreadable_history_confirmation_required',
    })
  })
})
