import { createSlice, PayloadAction } from '@reduxjs/toolkit'
import { isTerminalAttemptState } from '@/lib/receive-items'
import type { ReceiveSnapshot } from '@/lib/receive-items'
import type { RootState } from '../index'

export type TransferDirection = 'sending' | 'receiving'

export interface TransferProgressInfo {
  transferId: string
  entryId: string | null
  attemptId?: string | null
  peerId: string
  direction: TransferDirection
  bytesTransferred: number
  totalBytes: number | null
  status: 'active' | 'completed' | 'failed' | 'cancelled'
  errorMessage?: string
  /** Cancel reason sub-category — `local_user` / `remote_peer` / `replaced` / `timeout` / `unknown`. Only meaningful when `status === 'cancelled'`. */
  cancelReason?: string
  clipboardWriteCancelled?: boolean
  startedAt: number
  updatedAt: number
  bytesPerSecond: number | null
  estimatedRemainingSeconds: number | null
  itemProgress?: Record<string, { bytesTransferred: number; totalBytes: number | null }>
}

/** Durable entry-level transfer status seeded from command responses and status-changed events.
 *
 * `cancelled` 与 `failed` 在领域上是不同语义 —— 取消是预期内的用户/系统主动放弃,
 * UI 用中性灰色展示;失败是非预期错误,UI 用告警红色展示。reason 字段在 cancelled
 * 状态下是子原因 (`local_user` / `remote_peer` / `replaced` / `timeout` / `unknown`),
 * 用于映射 i18n 文案。
 */
export interface EntryTransferStatus {
  status: 'pending' | 'transferring' | 'completed' | 'failed' | 'cancelled'
  reason?: string | null
}

interface FileTransferState {
  activeTransfers: Record<string, TransferProgressInfo>
  entryTransferMap: Record<string, string>
  /** Durable entry-level transfer status keyed by entryId. Survives progress cleanup. */
  entryStatusById: Record<string, EntryTransferStatus>
  currentAttemptByEntry?: Record<string, string>
  /** Attempts replaced by a newer one; their late events and snapshots are ignored. */
  supersededAttemptsByEntry?: Record<string, string[]>
  /** Latest per-item receive snapshot of an entry, as read from the daemon. */
  receiveSnapshotByEntry?: Record<string, ReceiveSnapshot>
  /** Attempt whose cancellation was answered `cancellation_incomplete`, by entry. */
  cancelIncompleteByEntry?: Record<string, string>
}

/** Enough to cover several retries of one entry while bounding memory. */
const MAX_SUPERSEDED_ATTEMPTS = 16

const initialState: FileTransferState = {
  activeTransfers: {},
  entryTransferMap: {},
  entryStatusById: {},
  currentAttemptByEntry: {},
}

interface UpdateTransferProgressPayload {
  transferId: string
  entryId?: string | null
  attemptId?: string | null
  peerId: string
  direction: TransferDirection
  bytesTransferred: number
  totalBytes?: number | null
  eventTs?: number
}

/** Receive status shown for an attempt state; `undefined` keeps the previous status. */
export function entryStatusForAttemptState(
  attemptState: string
): EntryTransferStatus['status'] | undefined {
  if (attemptState === 'receiving' || attemptState === 'committing') return 'transferring'
  if (attemptState === 'completed') return 'completed'
  if (attemptState === 'cancelled') return 'cancelled'
  if (attemptState === 'failed') return 'failed'
  // `cancelling` and `failing` are not settled yet; the entry keeps its status.
  return undefined
}

function isSuperseded(state: FileTransferState, entryId: string, attemptId: string): boolean {
  return state.supersededAttemptsByEntry?.[entryId]?.includes(attemptId) ?? false
}

/** Make `attemptId` the current attempt, retiring the previous one for good. */
function adoptAttempt(state: FileTransferState, entryId: string, attemptId: string) {
  const current = state.currentAttemptByEntry?.[entryId]
  if (current && current !== attemptId) {
    const retired = ((state.supersededAttemptsByEntry ??= {})[entryId] ??= [])
    retired.push(current)
    if (retired.length > MAX_SUPERSEDED_ATTEMPTS) retired.shift()
    if (state.cancelIncompleteByEntry?.[entryId] === current) {
      delete state.cancelIncompleteByEntry[entryId]
    }
  }
  ;(state.currentAttemptByEntry ??= {})[entryId] = attemptId
}

function applyAttemptState(
  state: FileTransferState,
  entryId: string,
  attemptId: string,
  attemptState: string
) {
  const transferId = state.entryTransferMap[entryId]
  const transfer = transferId ? state.activeTransfers[transferId] : undefined
  if (transfer && transfer.attemptId === attemptId) {
    if (attemptState === 'completed') transfer.status = 'completed'
    if (attemptState === 'failed') transfer.status = 'failed'
    if (attemptState === 'cancelled') transfer.status = 'cancelled'
    transfer.updatedAt = Date.now()
  }
  const status = entryStatusForAttemptState(attemptState)
  if (status) state.entryStatusById[entryId] = { status, reason: null }
  if (
    isTerminalAttemptState(attemptState) &&
    state.cancelIncompleteByEntry?.[entryId] === attemptId
  ) {
    delete state.cancelIncompleteByEntry[entryId]
  }
}

const fileTransferSlice = createSlice({
  name: 'fileTransfer',
  initialState,
  reducers: {
    updateTransferProgress(state, action: PayloadAction<UpdateTransferProgressPayload>) {
      const {
        transferId,
        entryId,
        attemptId,
        peerId,
        direction,
        bytesTransferred,
        totalBytes,
        eventTs,
      } = action.payload
      if (
        entryId &&
        attemptId &&
        state.currentAttemptByEntry?.[entryId] &&
        state.currentAttemptByEntry[entryId] !== attemptId
      ) {
        return
      }
      if (entryId && attemptId) (state.currentAttemptByEntry ??= {})[entryId] = attemptId
      const aggregateId = entryId && attemptId ? `${entryId}\0${attemptId}` : transferId
      const now = eventTs ?? Date.now()
      const existing = state.activeTransfers[aggregateId]
      const itemProgress = {
        ...(existing?.itemProgress ?? {}),
        [transferId]: {
          bytesTransferred,
          totalBytes: totalBytes ?? null,
        },
      }
      const items = Object.values(itemProgress)
      const aggregateBytes = items.reduce((sum, item) => sum + item.bytesTransferred, 0)
      const aggregateTotal = items.every(item => item.totalBytes !== null)
        ? items.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0)
        : null

      const startedAt = existing?.startedAt ?? now
      const elapsedSeconds = Math.max((now - startedAt) / 1000, 0.001)
      const bytesPerSecond = aggregateBytes > 0 ? aggregateBytes / elapsedSeconds : null
      const totalBytesValue = aggregateTotal ?? existing?.totalBytes ?? null
      const estimatedRemainingSeconds =
        totalBytesValue && bytesPerSecond && bytesPerSecond > 0 && aggregateBytes <= totalBytesValue
          ? Math.max((totalBytesValue - aggregateBytes) / bytesPerSecond, 0)
          : null

      // Preserve terminal status set by status_changed events — progress events must not regress it.
      const status: TransferProgressInfo['status'] =
        existing?.status === 'completed' ||
        existing?.status === 'failed' ||
        existing?.status === 'cancelled'
          ? existing.status
          : 'active'

      state.activeTransfers[aggregateId] = {
        ...existing,
        transferId: aggregateId,
        entryId: entryId ?? existing?.entryId ?? null,
        attemptId: attemptId ?? existing?.attemptId ?? null,
        peerId,
        direction,
        bytesTransferred: aggregateBytes,
        totalBytes: totalBytesValue,
        status,
        startedAt,
        updatedAt: now,
        bytesPerSecond,
        estimatedRemainingSeconds,
        itemProgress,
      }

      if (entryId) {
        state.entryTransferMap[entryId] = aggregateId
      }
    },

    markTransferCompleted(state, action: PayloadAction<{ transferId: string }>) {
      const transfer = state.activeTransfers[action.payload.transferId]
      if (transfer) {
        transfer.status = 'completed'
        transfer.updatedAt = Date.now()
        transfer.estimatedRemainingSeconds = null
      }
    },

    linkTransferToEntry(state, action: PayloadAction<{ transferId: string; entryId: string }>) {
      const { transferId, entryId } = action.payload
      const transfer = state.activeTransfers[transferId]
      if (transfer) {
        transfer.entryId = entryId
      }
      state.entryTransferMap[entryId] = transferId
    },

    markTransferFailed(state, action: PayloadAction<{ transferId: string; error?: string }>) {
      const transfer = state.activeTransfers[action.payload.transferId]
      if (transfer) {
        transfer.status = 'failed'
        transfer.errorMessage = action.payload.error
        transfer.updatedAt = Date.now()
        transfer.estimatedRemainingSeconds = null
      }
    },

    /** Mark transfer as cancelled. Distinct from `markTransferFailed` so UI can
     * render a neutral cancelled state instead of an error indication. */
    markTransferCancelled(state, action: PayloadAction<{ transferId: string; reason?: string }>) {
      const transfer = state.activeTransfers[action.payload.transferId]
      if (transfer) {
        transfer.status = 'cancelled'
        transfer.cancelReason = action.payload.reason
        transfer.updatedAt = Date.now()
        transfer.estimatedRemainingSeconds = null
      }
    },

    cancelClipboardWrite(state) {
      // Cancel auto-clipboard-write for all active transfers when user copies something new
      for (const transfer of Object.values(state.activeTransfers)) {
        if (transfer.status === 'active') {
          transfer.clipboardWriteCancelled = true
        }
      }
    },

    clearCompletedTransfers(state) {
      const toRemove: string[] = []
      for (const [id, transfer] of Object.entries(state.activeTransfers)) {
        if (transfer.status === 'completed') {
          toRemove.push(id)
        }
      }
      for (const id of toRemove) {
        const transfer = state.activeTransfers[id]
        if (transfer?.entryId) {
          delete state.entryTransferMap[transfer.entryId]
        }
        delete state.activeTransfers[id]
      }
    },

    removeTransfer(state, action: PayloadAction<string>) {
      const transferId = action.payload
      const transfer = state.activeTransfers[transferId]
      if (transfer?.entryId) {
        delete state.entryTransferMap[transfer.entryId]
      }
      delete state.activeTransfers[transferId]
    },

    /** Seed or update durable entry-level transfer status from API responses or status-changed events. */
    setEntryTransferStatus(
      state,
      action: PayloadAction<{
        entryId: string
        status: EntryTransferStatus['status']
        reason?: string | null
      }>
    ) {
      const { entryId, status, reason } = action.payload
      state.entryStatusById[entryId] = { status, reason: reason ?? null }
    },

    /** Bulk-hydrate durable entry statuses from initial API query. */
    hydrateEntryTransferStatuses(
      state,
      action: PayloadAction<
        Array<{ entryId: string; status: EntryTransferStatus['status']; reason?: string | null }>
      >
    ) {
      for (const item of action.payload) {
        state.entryStatusById[item.entryId] = { status: item.status, reason: item.reason ?? null }
      }
    },

    /** Remove durable entry status (e.g., when entry is deleted). */
    removeEntryTransferStatus(state, action: PayloadAction<string>) {
      delete state.entryStatusById[action.payload]
      if (state.currentAttemptByEntry) delete state.currentAttemptByEntry[action.payload]
      if (state.supersededAttemptsByEntry) delete state.supersededAttemptsByEntry[action.payload]
      if (state.receiveSnapshotByEntry) delete state.receiveSnapshotByEntry[action.payload]
      if (state.cancelIncompleteByEntry) delete state.cancelIncompleteByEntry[action.payload]
    },

    setReceiveAttemptState(
      state,
      action: PayloadAction<{ entryId: string; attemptId: string; state: string }>
    ) {
      const { entryId, attemptId, state: attemptState } = action.payload
      // A retired attempt never comes back, whatever its late event says.
      if (isSuperseded(state, entryId, attemptId)) return
      const current = state.currentAttemptByEntry?.[entryId]
      if (current && current !== attemptId && attemptState !== 'receiving') return
      adoptAttempt(state, entryId, attemptId)
      applyAttemptState(state, entryId, attemptId, attemptState)
    },

    /**
     * Store a per-item snapshot read from the daemon. The snapshot is the
     * authority for which attempt is current; events only trigger the read.
     */
    applyReceiveSnapshot(state, action: PayloadAction<ReceiveSnapshot>) {
      const snapshot = action.payload
      const { entryId, attemptId } = snapshot
      if (isSuperseded(state, entryId, attemptId)) return
      const stored = state.receiveSnapshotByEntry?.[entryId]
      const settledStatus = state.entryStatusById[entryId]?.status
      const sameAttempt = state.currentAttemptByEntry?.[entryId] === attemptId
      // A reply that was produced before a settling event must not reopen the attempt.
      if (
        sameAttempt &&
        !isTerminalAttemptState(snapshot.state) &&
        (isTerminalAttemptState(stored?.state ?? '') ||
          settledStatus === 'completed' ||
          settledStatus === 'failed' ||
          settledStatus === 'cancelled')
      ) {
        return
      }
      adoptAttempt(state, entryId, attemptId)
      ;(state.receiveSnapshotByEntry ??= {})[entryId] = snapshot
      applyAttemptState(state, entryId, attemptId, snapshot.state)
    },

    /** The Engine is still releasing output or cleanup; the attempt is not cancelled yet. */
    markReceiveCancellationIncomplete(
      state,
      action: PayloadAction<{ entryId: string; attemptId: string }>
    ) {
      const { entryId, attemptId } = action.payload
      if (isSuperseded(state, entryId, attemptId)) return
      const current = state.currentAttemptByEntry?.[entryId]
      if (current && current !== attemptId) return
      ;(state.currentAttemptByEntry ??= {})[entryId] = attemptId
      ;(state.cancelIncompleteByEntry ??= {})[entryId] = attemptId
    },

    hydrateReceiveAttempts(
      state,
      action: PayloadAction<
        Array<{
          entryId: string
          attemptId: string
          state: string
          totalBytes: number
          completedBytes: number
        }>
      >
    ) {
      for (const receive of action.payload) {
        ;(state.currentAttemptByEntry ??= {})[receive.entryId] = receive.attemptId
        const transferId = `${receive.entryId}\0${receive.attemptId}`
        state.entryTransferMap[receive.entryId] = transferId
        state.activeTransfers[transferId] = {
          transferId,
          entryId: receive.entryId,
          attemptId: receive.attemptId,
          peerId: '',
          direction: 'receiving',
          bytesTransferred: receive.completedBytes,
          totalBytes: receive.totalBytes || null,
          status: 'active',
          startedAt: Date.now(),
          updatedAt: Date.now(),
          bytesPerSecond: null,
          estimatedRemainingSeconds: null,
          itemProgress: {},
        }
        state.entryStatusById[receive.entryId] = { status: 'transferring', reason: null }
      }
    },
  },
})

export const {
  updateTransferProgress,
  markTransferCompleted,
  linkTransferToEntry,
  markTransferFailed,
  markTransferCancelled,
  cancelClipboardWrite,
  clearCompletedTransfers,
  removeTransfer,
  setEntryTransferStatus,
  hydrateEntryTransferStatuses,
  removeEntryTransferStatus,
  setReceiveAttemptState,
  applyReceiveSnapshot,
  markReceiveCancellationIncomplete,
  hydrateReceiveAttempts,
} = fileTransferSlice.actions

// Selectors
export const selectTransferByEntryId = (
  state: RootState,
  entryId: string
): TransferProgressInfo | undefined => {
  const transferId = state.fileTransfer.entryTransferMap[entryId]
  if (!transferId) return undefined
  return state.fileTransfer.activeTransfers[transferId]
}

export const selectReceiveSnapshot = (
  state: RootState,
  entryId: string
): ReceiveSnapshot | undefined => state.fileTransfer.receiveSnapshotByEntry?.[entryId]

/** True while a cancel request was answered `cancellation_incomplete` for the current attempt. */
export const selectReceiveCancellationIncomplete = (state: RootState, entryId: string): boolean => {
  const attemptId = state.fileTransfer.cancelIncompleteByEntry?.[entryId]
  return !!attemptId && state.fileTransfer.currentAttemptByEntry?.[entryId] === attemptId
}

export const selectActiveTransfers = (state: RootState): TransferProgressInfo[] => {
  return Object.values(state.fileTransfer.activeTransfers).filter(t => t.status === 'active')
}

export const selectIsEntryTransferring = (state: RootState, entryId: string): boolean => {
  const transfer = selectTransferByEntryId(state, entryId)
  return transfer?.status === 'active'
}

/** Select durable entry-level transfer status (persisted, survives progress cleanup). */
export const selectEntryTransferStatus = (
  state: RootState,
  entryId: string
): EntryTransferStatus | undefined => {
  return state.fileTransfer.entryStatusById[entryId]
}

export function resolveEntryTransferStatus(
  entryStatus: EntryTransferStatus | undefined,
  transfer: TransferProgressInfo | undefined
): EntryTransferStatus['status'] | undefined {
  if (transfer?.status === 'failed') {
    return 'failed'
  }

  if (transfer?.status === 'cancelled') {
    return 'cancelled'
  }

  if (transfer?.status === 'active') {
    return 'transferring'
  }

  if (
    transfer?.status === 'completed' &&
    (!entryStatus || entryStatus.status === 'pending' || entryStatus.status === 'transferring')
  ) {
    return 'completed'
  }

  // 历史数据兼容:0.7.x 之前 DB 把 cancelled 落成 `status=failed` +
  // `reason='cancelled:<sub>'`,旧设备升级上来或者后端 fallback 表示这一档
  // 时,前端按 cancelled 渲染,与新数据视觉一致。
  if (
    entryStatus?.status === 'failed' &&
    typeof entryStatus.reason === 'string' &&
    entryStatus.reason.startsWith('cancelled:')
  ) {
    return 'cancelled'
  }

  return entryStatus?.status
}

/** Strip the legacy `cancelled:` prefix from a reason string, leaving only
 * the sub-category label (e.g. `cancelled:local_user` → `local_user`). New
 * server responses already send the bare label; this is the read-side
 * fallback for old DB rows. */
export function normalizeCancelReason(reason: string | null | undefined): string | undefined {
  if (!reason) return undefined
  return reason.startsWith('cancelled:') ? reason.slice('cancelled:'.length) : reason
}

export default fileTransferSlice.reducer
