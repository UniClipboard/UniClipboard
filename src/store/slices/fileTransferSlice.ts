import { createSlice, PayloadAction } from '@reduxjs/toolkit'
import type { RootState } from '../index'

export interface TransferProgressInfo {
  transferId: string
  entryId: string | null
  peerId: string
  direction: 'Sending' | 'Receiving'
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
}

const initialState: FileTransferState = {
  activeTransfers: {},
  entryTransferMap: {},
  entryStatusById: {},
}

interface UpdateTransferProgressPayload {
  transferId: string
  entryId?: string | null
  peerId: string
  direction: 'Sending' | 'Receiving'
  bytesTransferred: number
  totalBytes?: number | null
  eventTs?: number
}

const fileTransferSlice = createSlice({
  name: 'fileTransfer',
  initialState,
  reducers: {
    updateTransferProgress(state, action: PayloadAction<UpdateTransferProgressPayload>) {
      const { transferId, entryId, peerId, direction, bytesTransferred, totalBytes, eventTs } =
        action.payload
      const now = eventTs ?? Date.now()
      const existing = state.activeTransfers[transferId]

      const startedAt = existing?.startedAt ?? now
      const elapsedSeconds = Math.max((now - startedAt) / 1000, 0.001)
      const bytesPerSecond = bytesTransferred > 0 ? bytesTransferred / elapsedSeconds : null
      const totalBytesValue = totalBytes ?? existing?.totalBytes ?? null
      const estimatedRemainingSeconds =
        totalBytesValue &&
        bytesPerSecond &&
        bytesPerSecond > 0 &&
        bytesTransferred <= totalBytesValue
          ? Math.max((totalBytesValue - bytesTransferred) / bytesPerSecond, 0)
          : null

      // Preserve terminal status set by status_changed events — progress events must not regress it.
      const status: TransferProgressInfo['status'] =
        existing?.status === 'completed' ||
        existing?.status === 'failed' ||
        existing?.status === 'cancelled'
          ? existing.status
          : 'active'

      state.activeTransfers[transferId] = {
        ...existing,
        transferId,
        entryId: entryId ?? existing?.entryId ?? null,
        peerId,
        direction,
        bytesTransferred,
        totalBytes: totalBytesValue,
        status,
        startedAt,
        updatedAt: now,
        bytesPerSecond,
        estimatedRemainingSeconds,
      }

      if (entryId) {
        state.entryTransferMap[entryId] = transferId
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
