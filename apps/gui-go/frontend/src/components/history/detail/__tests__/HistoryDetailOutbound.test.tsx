import { configureStore } from '@reduxjs/toolkit'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { describe, expect, it } from 'vitest'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import outboundProgressReducer, {
  replaceOutboundSnapshots,
} from '@/store/slices/outboundProgressSlice'
import HistoryDetailOutbound from '../HistoryDetailOutbound'

function snapshot(patch: Partial<OutboundProgressSnapshot>): OutboundProgressSnapshot {
  return {
    entryId: 'e1',
    targetDeviceId: 'dev-a',
    attemptId: 'attempt-1',
    revision: 1,
    freshness: 'live',
    freshnessAvailable: true,
    state: 'in_progress',
    itemsTotal: 1,
    itemsWaiting: 0,
    itemsRunning: 1,
    itemsCompleted: 0,
    itemsFailed: 0,
    itemsCancelled: 0,
    itemsUnconfirmed: 0,
    items: [
      {
        kind: 'file',
        index: 0,
        state: 'transferring',
        completedBytes: 10,
        totalBytes: null,
      },
    ],
    ...patch,
  }
}

function renderWith(snapshots: OutboundProgressSnapshot[]) {
  const store = configureStore({ reducer: { outboundProgress: outboundProgressReducer } })
  store.dispatch(replaceOutboundSnapshots(snapshots))
  return render(
    <Provider store={store as never}>
      <HistoryDetailOutbound entryId="e1" deviceNames={{ 'dev-a': 'Laptop', 'dev-b': 'Phone' }} />
    </Provider>
  )
}

describe('HistoryDetailOutbound', () => {
  it('renders one row per target and keeps them apart', () => {
    renderWith([
      snapshot({ targetDeviceId: 'dev-a', state: 'completed', freshness: 'settled' }),
      snapshot({
        targetDeviceId: 'dev-b',
        state: 'failed',
        freshness: 'settled',
        itemsFailed: 1,
        items: [
          { kind: 'file', index: 0, state: 'failed', failureReason: 'timed_out', totalBytes: 5 },
        ],
      }),
    ])
    expect(screen.getByText('Laptop')).toBeTruthy()
    expect(screen.getByText('Phone')).toBeTruthy()
    const rows = document.querySelectorAll('[data-outbound-state]')
    expect(Array.from(rows).map(row => row.getAttribute('data-outbound-state'))).toEqual([
      'completed',
      'failed',
    ])
  })

  it('shows an unknown total as indeterminate, not as 0%', () => {
    renderWith([snapshot({})])
    const bar = screen.getByRole('progressbar')
    expect(bar.getAttribute('aria-valuenow')).toBeNull()
  })

  it('never animates an unconfirmed snapshot', () => {
    renderWith([snapshot({ state: 'unconfirmed', freshness: 'unconfirmed' })])
    expect(document.querySelector('.animate-spin')).toBeNull()
    expect(document.querySelector('.animate-pulse')).toBeNull()
  })

  it('renders nothing without snapshots', () => {
    const { container } = renderWith([])
    expect(container.firstChild).toBeNull()
  })
})
