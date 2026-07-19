import { beforeEach, describe, expect, it, vi } from 'vitest'
import { cancelEntryReceive, cancelFileTransfer, listEntryReceives } from '@/api/file_transfer'
import {
  cancelClipboardTransfer,
  cancelEntryReceive as cancelEntryReceiveSdk,
  listEntryReceiveProgress,
} from '@/api/generated/sdk.gen'

// ADR-008 P7: file-transfer cancellation routes through the generated SDK +
// daemonClient.callSdk. Mock callSdk to faithfully replicate its happy path
// (invoke the thunk, unwrap the `{ data }` envelope), and mock the generated SDK
// fn so we can assert the request shape and control its enveloped response.
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    request: vi.fn(),
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    // 复刻 callEnveloped 快乐路径：连拆 SDK { data } 与 { data, ts } 信封。
    callEnveloped: vi.fn((call: () => Promise<{ data: { data: unknown } }>) =>
      call().then(r => r.data.data)
    ),
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  cancelClipboardTransfer: vi.fn(),
  cancelEntryReceive: vi.fn(),
  listEntryReceiveProgress: vi.fn(),
}))

const cancelSdkMock = cancelClipboardTransfer as unknown as ReturnType<typeof vi.fn>
const cancelEntrySdkMock = cancelEntryReceiveSdk as unknown as ReturnType<typeof vi.fn>
const listReceivesSdkMock = listEntryReceiveProgress as unknown as ReturnType<typeof vi.fn>

beforeEach(() => {
  cancelSdkMock.mockReset()
  cancelEntrySdkMock.mockReset()
  listReceivesSdkMock.mockReset()
})

describe('cancelFileTransfer', () => {
  it('calls the daemon cancel-transfer endpoint with the local-user reason', async () => {
    cancelSdkMock.mockResolvedValueOnce({ data: { outcome: 'cancelled' } })

    await cancelFileTransfer('transfer-1')

    expect(cancelSdkMock).toHaveBeenCalledWith({
      path: { transfer_id: 'transfer-1' },
      body: { reason: 'local_user' },
      throwOnError: true,
    })
  })
})

describe('cancelEntryReceive', () => {
  it('sends both the entry and expected attempt identity', async () => {
    cancelEntrySdkMock.mockResolvedValueOnce({
      data: { data: { outcome: 'cancellation_requested' }, ts: 1 },
    })

    await expect(cancelEntryReceive('entry-1', 'attempt-2')).resolves.toBe('cancellation_requested')
    expect(cancelEntrySdkMock).toHaveBeenCalledWith({
      path: { id: 'entry-1' },
      body: { attemptId: 'attempt-2' },
      throwOnError: true,
    })
  })
})

describe('listEntryReceives', () => {
  it('hydrates active receives through the generated daemon contract', async () => {
    const receives = [
      {
        entryId: 'entry-1',
        attemptId: 'attempt-2',
        state: 'receiving',
        totalBytes: 20,
        completedBytes: 10,
        itemsTotal: 2,
        itemsCompleted: 1,
      },
    ]
    listReceivesSdkMock.mockResolvedValueOnce({ data: { data: receives, ts: 1 } })

    await expect(listEntryReceives()).resolves.toEqual(receives)
    expect(listReceivesSdkMock).toHaveBeenCalledWith({ throwOnError: true })
  })
})
