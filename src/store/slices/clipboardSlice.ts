import { createSlice, createAsyncThunk, PayloadAction } from '@reduxjs/toolkit'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'

// ── State ────────────────────────────────────────────────────────
//
// The browse list (`items`) used to live here, fed by the list endpoint via
// `useClipboardEvents`. Browse is now served by the unified live search
// (`useLiveSearch`, used by the history page and quick panel), so this slice only
// retains the inbound-transfer placeholder overlay plus a few action thunks the
// history page / settings still call for their side effects.

/**
 * Inbound clipboard entry that the daemon has acknowledged but not yet
 * fully fetched/persisted. Surfaced via `clipboard.incoming_pending`
 * events so the UI can render a placeholder card with a live progress
 * bar before the real entry lands. Cleared as soon as the matching
 * `clipboard.new_content` event arrives.
 */
export interface PendingClipboardEntry {
  entryId: string
  fromDevice: string
  totalBytes: number | null
  /**
   * Filenames advertised in the V3 envelope (free-standing files only).
   * Empty when the inbound entry is text-only or pure image/binary blobs
   * with no associated filename.
   */
  filenames: string[]
  createdAt: number
}

interface ClipboardState {
  pendingItems: PendingClipboardEntry[]
}

const initialState: ClipboardState = {
  pendingItems: [],
}

// ── Action thunks (API side effects; the live search owns list state) ─────────

export const removeClipboardItem = createAsyncThunk(
  'clipboard/removeItem',
  async (id: string, { rejectWithValue }) => {
    try {
      // Daemon API: DELETE /clipboard/entries/:id
      await deleteClipboardEntry(id)
      return id
    } catch {
      return rejectWithValue('删除剪贴板内容失败')
    }
  }
)

export const clearAllItems = createAsyncThunk(
  'clipboard/clearAll',
  async (_, { rejectWithValue }) => {
    try {
      // Daemon API: POST /clipboard/entries/clear
      const { clearClipboardHistory } = await import('@/api/daemon/clipboard')
      await clearClipboardHistory()
      return true
    } catch {
      return rejectWithValue('清空剪贴板内容失败')
    }
  }
)

export const copyToClipboard = createAsyncThunk(
  'clipboard/copyItem',
  async (id: string, { rejectWithValue }) => {
    try {
      // Daemon API: POST /clipboard/restore/:id
      await restoreClipboardEntry(id)
      return { id, success: true }
    } catch (error) {
      // 410 Gone → bytes for this entry are gone (orphaned Staged that has been
      // demoted to Lost). Don't retry. Surface a distinct rejection so the UI
      // can render a "content unavailable" message and stop hammering the
      // daemon endpoint.
      const { DaemonApiError, DaemonErrorCode } = await import('@/api/daemon/errors')
      if (error instanceof DaemonApiError && error.code === DaemonErrorCode.PAYLOAD_UNAVAILABLE) {
        return rejectWithValue('该内容已不可用（数据已丢失），请从历史中删除该条目')
      }
      return rejectWithValue('复制到剪贴板失败')
    }
  }
)

const clipboardSlice = createSlice({
  name: 'clipboard',
  initialState,
  reducers: {
    /**
     * Insert a pending entry placeholder. Idempotent on entryId — repeated
     * inbound events for the same id replace the previous entry rather
     * than creating duplicates.
     */
    addPendingEntry: (state, action: PayloadAction<PendingClipboardEntry>) => {
      const incoming = action.payload
      const existingIndex = state.pendingItems.findIndex(p => p.entryId === incoming.entryId)
      if (existingIndex >= 0) {
        state.pendingItems[existingIndex] = incoming
      } else {
        state.pendingItems.unshift(incoming)
      }
    },
    /**
     * Drop the placeholder for `entryId`. Called when `clipboard.new_content`
     * arrives — the real entry surfaces through the live search list.
     */
    removePendingEntry: (state, action: PayloadAction<string>) => {
      state.pendingItems = state.pendingItems.filter(p => p.entryId !== action.payload)
    },
  },
})

export const { addPendingEntry, removePendingEntry } = clipboardSlice.actions

export default clipboardSlice.reducer
