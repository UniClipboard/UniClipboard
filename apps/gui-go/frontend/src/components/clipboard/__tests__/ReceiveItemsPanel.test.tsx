import { configureStore } from '@reduxjs/toolkit'
import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getEntryReceive, retryEntryReceive } from '@/api/file_transfer'
import { daemonWs } from '@/lib/daemon-ws'
import type { ReceiveSnapshot } from '@/lib/receive-items'
import fileTransferReducer, {
  markReceiveCancellationIncomplete,
} from '@/store/slices/fileTransferSlice'
import ReceiveItemsPanel from '../ReceiveItemsPanel'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && 'count' in opts ? `${key}:${String(opts.count)}` : key,
  }),
}))

vi.mock('@/api/file_transfer', () => ({
  getEntryReceive: vi.fn(),
  retryEntryReceive: vi.fn(),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: { subscribe: vi.fn(() => vi.fn()), onReconnect: vi.fn(() => vi.fn()) },
}))

vi.mock('@/observability/errors', () => ({ reportError: vi.fn() }))

const getMock = vi.mocked(getEntryReceive)
const retryMock = vi.mocked(retryEntryReceive)

function snap(attemptId: string, state: string, items: ReceiveSnapshot['items']): ReceiveSnapshot {
  return {
    entryId: 'entry-1',
    attemptId,
    state,
    totalBytes: 300,
    completedBytes: 100,
    itemsTotal: items.length,
    itemsCompleted: items.filter(item => item.state === 'completed').length,
    items,
  }
}
const file = (itemId: string, state: string, failureReason: string | null = null) => ({
  itemId,
  kind: 'file',
  state,
  failureReason,
})

function renderPanel() {
  const store = configureStore({ reducer: { fileTransfer: fileTransferReducer } })
  render(
    <Provider store={store}>
      <ReceiveItemsPanel entryId="entry-1" />
    </Provider>
  )
  return store
}

beforeEach(() => {
  getMock.mockReset()
  retryMock.mockReset()
})

describe('ReceiveItemsPanel', () => {
  it('shows each file result and keeps the saved file saved next to the failed one', async () => {
    getMock.mockResolvedValue(
      snap('a1', 'failed', [file('x', 'completed'), file('y', 'failed', 'timed_out')])
    )
    renderPanel()

    const rows = await screen.findAllByTestId('receive-item')
    expect(rows.map(row => row.getAttribute('data-state'))).toEqual(['completed', 'failed'])
    expect(rows[1]).toHaveTextContent('clipboard.receive.reason.timed_out')
  })

  it('renders nothing for an entry without a receive', async () => {
    getMock.mockResolvedValue(null)
    renderPanel()
    await waitFor(() => expect(getMock).toHaveBeenCalled())
    expect(screen.queryByTestId('receive-items')).toBeNull()
  })

  it('retries only the failed file, against the attempt that was queried', async () => {
    getMock.mockResolvedValue(
      snap('a1', 'failed', [file('x', 'completed'), file('y', 'failed'), file('z', 'cancelled')])
    )
    retryMock.mockResolvedValue({ outcome: 'accepted', attemptId: 'a2', reason: null })
    renderPanel()

    await userEvent.click(
      await screen.findByRole('button', { name: 'clipboard.receive.retry.action' })
    )

    expect(retryMock).toHaveBeenCalledWith('entry-1', 'a1', ['y', 'z'])
  })

  it('does not call an accepted retry delivered: the saved files stay and the new attempt is awaited', async () => {
    getMock
      .mockResolvedValueOnce(snap('a1', 'failed', [file('x', 'completed'), file('y', 'failed')]))
      .mockResolvedValue(
        snap('a2', 'receiving', [file('x', 'completed'), file('y', 'transferring')])
      )
    retryMock.mockResolvedValue({ outcome: 'accepted', attemptId: 'a2', reason: null })
    renderPanel()

    await userEvent.click(
      await screen.findByRole('button', { name: 'clipboard.receive.retry.action' })
    )

    await waitFor(() => {
      const rows = screen.getAllByTestId('receive-item')
      expect(rows.map(row => row.getAttribute('data-state'))).toEqual(['completed', 'transferring'])
    })
    expect(screen.queryByRole('button', { name: 'clipboard.receive.retry.action' })).toBeNull()
  })

  it('reports an unavailable source honestly and leaves the results unchanged', async () => {
    getMock.mockResolvedValue(snap('a1', 'failed', [file('x', 'completed'), file('y', 'failed')]))
    retryMock.mockResolvedValue({
      outcome: 'unavailable',
      attemptId: null,
      reason: 'source_unavailable',
    })
    renderPanel()

    await userEvent.click(
      await screen.findByRole('button', { name: 'clipboard.receive.retry.action' })
    )

    expect(await screen.findByText('clipboard.receive.retry.sourceUnavailable')).toBeInTheDocument()
    expect(
      screen.getAllByTestId('receive-item').map(row => row.getAttribute('data-state'))
    ).toEqual(['completed', 'failed'])
  })

  it('offers no retry while the attempt is still running', async () => {
    getMock.mockResolvedValue(
      snap('a1', 'receiving', [file('x', 'completed'), file('y', 'failed')])
    )
    renderPanel()
    await screen.findAllByTestId('receive-item')
    expect(screen.queryByRole('button', { name: 'clipboard.receive.retry.action' })).toBeNull()
  })

  it('queries again when an event says the attempt changed', async () => {
    getMock.mockResolvedValueOnce(snap('a1', 'receiving', [file('y', 'transferring')]))
    getMock.mockResolvedValue(snap('a1', 'failed', [file('y', 'failed')]))
    renderPanel()
    await screen.findAllByTestId('receive-item')

    act(() => {
      const callback = vi.mocked(daemonWs.subscribe).mock.lastCall![1]
      callback({
        eventType: 'clipboard.receive_attempt_state_changed',
        payload: { entryId: 'entry-1', attemptId: 'a1', state: 'failed' },
      })
    })

    await waitFor(() =>
      expect(screen.getByTestId('receive-item').getAttribute('data-state')).toBe('failed')
    )
    expect(getMock).toHaveBeenCalledTimes(2)
  })

  it('shows an incomplete cancellation as still stopping, not as cancelled', async () => {
    getMock.mockResolvedValue(snap('a1', 'receiving', [file('y', 'transferring')]))
    const store = renderPanel()
    await screen.findAllByTestId('receive-item')

    act(() => {
      store.dispatch(markReceiveCancellationIncomplete({ entryId: 'entry-1', attemptId: 'a1' }))
    })

    expect(await screen.findByText('clipboard.receive.cancelIncomplete')).toBeInTheDocument()
    expect(screen.queryByText('clipboard.transfer.cancelled')).toBeNull()
  })
})
