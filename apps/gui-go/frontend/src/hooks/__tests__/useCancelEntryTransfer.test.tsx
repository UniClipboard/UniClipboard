import { configureStore } from '@reduxjs/toolkit'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { cancelEntryReceive, cancelFileTransfer } from '@/api/file_transfer'
import fileTransferReducer, {
  selectReceiveCancellationIncomplete,
} from '@/store/slices/fileTransferSlice'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { useCancelEntryTransfer } from '../useCancelEntryTransfer'

vi.mock('@/api/file_transfer', () => ({
  cancelEntryReceive: vi.fn(),
  cancelFileTransfer: vi.fn(),
}))
vi.mock('@/observability/errors', () => ({ reportError: vi.fn() }))

const cancelReceive = vi.mocked(cancelEntryReceive)
const cancelTransfer = vi.mocked(cancelFileTransfer)

const transfer = {
  transferId: 'entry-1\0a1',
  entryId: 'entry-1',
  attemptId: 'a1',
  peerId: '',
  direction: 'receiving',
  bytesTransferred: 0,
  totalBytes: 10,
  status: 'active',
  startedAt: 0,
  updatedAt: 0,
  bytesPerSecond: null,
  estimatedRemainingSeconds: null,
} satisfies TransferProgressInfo

function setup() {
  const store = configureStore({ reducer: { fileTransfer: fileTransferReducer } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <Provider store={store}>{children}</Provider>
  )
  const hook = renderHook(() => useCancelEntryTransfer('entry-1', transfer), { wrapper })
  return { store, hook }
}

beforeEach(() => {
  cancelReceive.mockReset()
  cancelTransfer.mockReset()
})

describe('useCancelEntryTransfer', () => {
  it('records an incomplete cancellation instead of treating it as cancelled', async () => {
    cancelReceive.mockResolvedValue('cancellation_incomplete')
    const { store, hook } = setup()

    await act(async () => {
      await hook.result.current.cancel()
    })

    expect(
      selectReceiveCancellationIncomplete(
        store.getState() as Parameters<typeof selectReceiveCancellationIncomplete>[0],
        'entry-1'
      )
    ).toBe(true)
  })

  it('leaves no incomplete marker after a confirmed cancellation', async () => {
    cancelReceive.mockResolvedValue('cancelled')
    const { store, hook } = setup()

    await act(async () => {
      await hook.result.current.cancel()
    })

    expect(
      selectReceiveCancellationIncomplete(
        store.getState() as Parameters<typeof selectReceiveCancellationIncomplete>[0],
        'entry-1'
      )
    ).toBe(false)
  })
})
