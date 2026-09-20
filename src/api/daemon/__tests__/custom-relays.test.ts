import { beforeEach, describe, expect, it, vi } from 'vitest'
import { CustomRelayMutationError, getCustomRelays, mutateCustomRelay } from '@/api/daemon/settings'
import {
  getCustomRelays as getCustomRelaysSdk,
  mutateCustomRelay as mutateCustomRelaySdk,
} from '@/api/generated/sdk.gen'

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    callEnveloped: vi.fn((call: () => Promise<{ data: { data: unknown } }>) =>
      call().then(response => response.data.data)
    ),
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  getCustomRelays: vi.fn(),
  mutateCustomRelay: vi.fn(),
}))

const getCustomRelaysSdkMock = vi.mocked(getCustomRelaysSdk)
const mutateCustomRelaySdkMock = vi.mocked(mutateCustomRelaySdk)

beforeEach(() => {
  vi.clearAllMocks()
})

describe('custom relay API', () => {
  it('returns the authoritative normalized list and credential state', async () => {
    const relays = [
      { url: 'https://relay-a.example.com/', credentialConfigured: true },
      { url: 'https://relay-b.example.com/', credentialConfigured: false },
    ]
    getCustomRelaysSdkMock.mockResolvedValueOnce({
      data: { data: relays, ts: 0 },
    } as never)

    await expect(getCustomRelays()).resolves.toEqual(relays)
    expect(getCustomRelaysSdkMock).toHaveBeenCalledWith({ throwOnError: true })
  })

  it('sends one edit intent and keeps the stored credential when no replacement is supplied', async () => {
    const relays = [{ url: 'https://relay-new.example.com/', credentialConfigured: true }]
    mutateCustomRelaySdkMock.mockResolvedValueOnce({
      data: { data: { relays }, ts: 0 },
    } as never)

    await expect(
      mutateCustomRelay({
        action: 'edit',
        previousUrl: 'https://relay-old.example.com/',
        url: 'https://relay-new.example.com',
        credential: { action: 'keep' },
      })
    ).resolves.toEqual({ relays })

    expect(mutateCustomRelaySdkMock).toHaveBeenCalledWith({
      body: {
        action: 'edit',
        previousUrl: 'https://relay-old.example.com/',
        url: 'https://relay-new.example.com',
        credential: { action: 'keep' },
      },
      throwOnError: true,
    })
  })

  it.each([
    ['custom_relay_invalid_url', 'invalidUrl'],
    ['custom_relay_duplicate', 'duplicate'],
    ['custom_relay_not_found', 'notFound'],
  ] as const)('maps %s to the stable %s rejection', async (code, kind) => {
    mutateCustomRelaySdkMock.mockRejectedValueOnce({
      code,
      message: 'internal engine text',
    })

    await expect(
      mutateCustomRelay({
        action: 'delete',
        url: 'https://relay.example.com/',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<CustomRelayMutationError>>({ kind }))
  })
})
