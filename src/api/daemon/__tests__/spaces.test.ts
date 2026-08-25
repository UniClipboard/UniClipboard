import { beforeEach, describe, expect, it, vi } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import {
  createSpaceProfile,
  deleteSpaceProfile,
  joinSpaceProfile,
  listSpaces,
  setActiveSendSpace,
} from '@/api/daemon/spaces'

const request = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/client', () => ({
  daemonClient: { request },
}))

const runningSpaceWire = {
  profileId: 'profile-a',
  spaceId: 'space-a',
  displayName: 'Work',
  deviceName: 'Office PC',
  runtimeState: { state: 'running' },
  incomingSyncState: { state: 'receiving' },
  lastFault: null,
  isActiveSend: true,
}

describe('spaces daemon API', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('lists spaces through the canonical v2 envelope and tolerates future fields', async () => {
    request.mockResolvedValue({
      data: [
        {
          ...runningSpaceWire,
          futureSummaryField: 'ignored',
          runtimeState: { state: 'running', futureRuntimeField: true },
        },
      ],
      ts: 123,
      futureEnvelopeField: 'ignored',
    })

    await expect(listSpaces()).resolves.toEqual([runningSpaceWire])
    expect(request).toHaveBeenCalledWith('/v2/spaces', { method: 'GET' })
  })

  it('uses the exact Rust wire requests for create, join, active-send, and delete', async () => {
    request.mockResolvedValue({ data: runningSpaceWire, ts: 123 })

    await createSpaceProfile({
      passphrase: 'correct horse battery staple',
      passphraseConfirm: 'correct horse battery staple',
      deviceName: 'Office PC',
    })
    await joinSpaceProfile({
      code: 'ABCD-1234',
      passphrase: 'correct horse battery staple',
      deviceName: null,
    })
    await setActiveSendSpace('profile-a')
    await expect(deleteSpaceProfile('profile/a')).resolves.toEqual(runningSpaceWire)

    expect(request).toHaveBeenNthCalledWith(1, '/v2/spaces', {
      method: 'POST',
      body: {
        passphrase: 'correct horse battery staple',
        passphraseConfirm: 'correct horse battery staple',
        deviceName: 'Office PC',
      },
    })
    expect(request).toHaveBeenNthCalledWith(2, '/v2/spaces/join', {
      method: 'POST',
      body: {
        code: 'ABCD-1234',
        passphrase: 'correct horse battery staple',
        deviceName: null,
      },
    })
    expect(request).toHaveBeenNthCalledWith(3, '/v2/spaces/active-send', {
      method: 'PUT',
      body: { profileId: 'profile-a' },
    })
    expect(request).toHaveBeenNthCalledWith(4, '/v2/spaces/profile%2Fa', {
      method: 'DELETE',
    })
  })

  it('requires DELETE to return the 200 summary envelope rather than an empty response', async () => {
    request.mockResolvedValue(undefined)

    await expect(deleteSpaceProfile('profile-a')).rejects.toBeInstanceOf(DaemonApiError)
  })

  it('rejects a response missing a required profile field with the shared error type', async () => {
    const { isActiveSend: _missing, ...malformed } = runningSpaceWire
    request.mockResolvedValue({ data: [malformed], ts: 123 })

    await expect(listSpaces()).rejects.toMatchObject({
      name: 'DaemonApiError',
      code: DaemonErrorCode.INTERNAL_ERROR,
    })
  })

  it('rejects unknown runtime discriminators instead of guessing a state', async () => {
    request.mockResolvedValue({
      data: [{ ...runningSpaceWire, runtimeState: { state: 'future_magic' } }],
      ts: 123,
    })

    await expect(listSpaces()).rejects.toBeInstanceOf(DaemonApiError)
  })
})
