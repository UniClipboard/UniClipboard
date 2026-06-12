import { beforeEach, describe, expect, it, vi } from 'vitest'
import { cancelFileTransfer } from '@/api/file_transfer'
import { cancelClipboardTransfer } from '@/api/generated/sdk.gen'

// ADR-008 P7: file-transfer cancellation routes through the generated SDK +
// daemonClient.callSdk. Mock callSdk to faithfully replicate its happy path
// (invoke the thunk, unwrap the `{ data }` envelope), and mock the generated SDK
// fn so we can assert the request shape and control its enveloped response.
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    callSdk: vi.fn((call: () => Promise<{ data: unknown }>) => call().then(r => r.data)),
    // 复刻 callEnveloped 快乐路径：连拆 SDK { data } 与 { data, ts } 信封。
    callEnveloped: vi.fn((call: () => Promise<{ data: { data: unknown } }>) =>
      call().then(r => r.data.data)
    ),
  },
}))

vi.mock('@/api/generated/sdk.gen', () => ({
  cancelClipboardTransfer: vi.fn(),
}))

const cancelSdkMock = cancelClipboardTransfer as unknown as ReturnType<typeof vi.fn>

beforeEach(() => {
  cancelSdkMock.mockReset()
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
