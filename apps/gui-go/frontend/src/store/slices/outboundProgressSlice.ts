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
