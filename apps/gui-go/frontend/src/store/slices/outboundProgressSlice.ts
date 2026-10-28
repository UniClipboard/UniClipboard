import { createSlice, PayloadAction } from '@reduxjs/toolkit'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import type { RootState } from '../index'

/**
 * Sender-side outbound progress, keyed by entry and target device.
 *
 * The cache is rebuilt from the authoritative snapshot list and never patched
 * from events or timers: a target that is absent from the latest list has no
 * task, and a stale one is not shown as running.
 */
interface OutboundProgressState {
  byEntry: Record<string, Record<string, OutboundProgressSnapshot>>
}

const initialState: OutboundProgressState = { byEntry: {} }

const outboundProgressSlice = createSlice({
  name: 'outboundProgress',
  initialState,
  reducers: {
    /** Replace the whole cache with one authoritative list read. */
    replaceOutboundSnapshots(state, action: PayloadAction<OutboundProgressSnapshot[]>) {
      const byEntry: OutboundProgressState['byEntry'] = {}
      for (const snapshot of action.payload) {
        const targets = (byEntry[snapshot.entryId] ??= {})
        const previous = targets[snapshot.targetDeviceId]
        // A late duplicate of an older revision must not win.
        if (!previous || snapshot.revision >= previous.revision) {
          targets[snapshot.targetDeviceId] = snapshot
        }
      }
      state.byEntry = byEntry
    },
  },
})

export const { replaceOutboundSnapshots } = outboundProgressSlice.actions

const EMPTY: OutboundProgressSnapshot[] = []

/**
 * Only a live, waiting or running attempt counts as activity. An unconfirmed or
 * settled snapshot never does: the host does not guess an outcome from a timer.
 */
export function isOutboundRunning(snapshot: OutboundProgressSnapshot): boolean {
  return (
    snapshot.freshness !== 'unconfirmed' &&
    snapshot.freshness !== 'settled' &&
    (snapshot.state === 'in_progress' || snapshot.state === 'waiting')
  )
}

export interface OutboundActivity {
  running: boolean
  /** Bytes reported so far across running targets (unreported items count as 0). */
  bytes: number
  /** `null` unless every item of every running target reports a total (unknown is not 0). */
  total: number | null
}

export const IDLE_OUTBOUND: OutboundActivity = { running: false, bytes: 0, total: null }

/** Entry-level activity for the history list, derived only from the snapshot cache. */
export function selectOutboundActivity(state: RootState, entryId: string): OutboundActivity {
  const targets = state.outboundProgress.byEntry[entryId]
  if (!targets) return IDLE_OUTBOUND
  const running = Object.values(targets).filter(isOutboundRunning)
  if (running.length === 0) return IDLE_OUTBOUND
  const items = running.flatMap(snapshot => snapshot.items ?? [])
  const known = items.length > 0 && items.every(item => item.totalBytes != null)
  return {
    running: true,
    bytes: items.reduce((sum, item) => sum + (item.completedBytes ?? 0), 0),
    total: known ? items.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0) : null,
  }
}

export const selectOutboundByEntry = (
  state: RootState,
  entryId: string
): Record<string, OutboundProgressSnapshot> | undefined => state.outboundProgress.byEntry[entryId]

export function selectOutboundList(
  targets: Record<string, OutboundProgressSnapshot> | undefined
): OutboundProgressSnapshot[] {
  return targets ? Object.values(targets) : EMPTY
}

export default outboundProgressSlice.reducer
