import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getEntryReceive, retryEntryReceive } from '@/api/file_transfer'
import {
  getEntryReceiveProgress,
  retryEntryReceive as retryEntryReceiveSdk,
} from '@/api/generated/sdk.gen'

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    callEnveloped: vi.fn((call: () => Promise<{ data: { data: unknown } }>) =>
      call().then(r => r.data.data)
    ),
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  cancelClipboardTransfer: vi.fn(),
  cancelEntryReceive: vi.fn(),
  listEntryReceiveProgress: vi.fn(),
  getEntryReceiveProgress: vi.fn(),
  retryEntryReceive: vi.fn(),
}))

const getSdk = getEntryReceiveProgress as unknown as ReturnType<typeof vi.fn>
const retrySdk = retryEntryReceiveSdk as unknown as ReturnType<typeof vi.fn>

beforeEach(() => {
  getSdk.mockReset()
  retrySdk.mockReset()
})

describe('getEntryReceive', () => {
  it('reads the authoritative snapshot of one entry, including per-file results', async () => {
    const snapshot = {
      entryId: 'entry-1',
      attemptId: 'attempt-1',
      state: 'failed',
      totalBytes: 20,
      completedBytes: 10,
      itemsTotal: 2,
      itemsCompleted: 1,
      items: [
        { itemId: 'a', kind: 'file', state: 'completed', failureReason: null },
        { itemId: 'b', kind: 'file', state: 'failed', failureReason: 'timed_out' },
      ],
    }
    getSdk.mockResolvedValueOnce({ data: { data: snapshot, ts: 1 } })

    await expect(getEntryReceive('entry-1')).resolves.toEqual(snapshot)
    expect(getSdk).toHaveBeenCalledWith({ path: { id: 'entry-1' }, throwOnError: true })
  })

  it('returns null when the entry has no receive', async () => {
    getSdk.mockResolvedValueOnce({ data: { data: null, ts: 1 } })
    await expect(getEntryReceive('entry-1')).resolves.toBeNull()
  })

  it('keeps an old daemon without items readable as an empty list', async () => {
    getSdk.mockResolvedValueOnce({
      data: {
        data: {
          entryId: 'entry-1',
          attemptId: 'attempt-1',
          state: 'receiving',
          totalBytes: 20,
          completedBytes: 10,
          itemsTotal: 2,
          itemsCompleted: 1,
        },
        ts: 1,
      },
    })
    const snapshot = await getEntryReceive('entry-1')
    expect(snapshot?.items).toEqual([])
  })
})

describe('retryEntryReceive', () => {
  it('sends the entry, the expected attempt and the selected item ids from the query', async () => {
    retrySdk.mockResolvedValueOnce({
      data: { data: { outcome: 'accepted', attemptId: 'attempt-2', reason: null }, ts: 1 },
    })

    await expect(retryEntryReceive('entry-1', 'attempt-1', ['b'])).resolves.toEqual({
      outcome: 'accepted',
      attemptId: 'attempt-2',
      reason: null,
    })
    expect(retrySdk).toHaveBeenCalledWith({
      path: { id: 'entry-1' },
      body: { expectedAttemptId: 'attempt-1', itemIds: ['b'] },
      throwOnError: true,
    })
  })

  it('refuses to send an empty selection', async () => {
    await expect(retryEntryReceive('entry-1', 'attempt-1', [])).rejects.toThrow()
    expect(retrySdk).not.toHaveBeenCalled()
  })
})
