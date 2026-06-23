import { createSlice, createAsyncThunk, PayloadAction } from '@reduxjs/toolkit'
import { OrderBy, Filter } from '@/api/clipboardItems'
import {
  getClipboardEntries,
  deleteClipboardEntry,
  restoreClipboardEntry,
  toggleFavorite,
} from '@/api/daemon'
import type { ClipboardEntry } from '@/lib/clipboard-entry'
import { projectClipboardEntry } from '@/lib/clipboard-transform'
import { hydrateEntryTransferStatuses } from './fileTransferSlice'

// ── State ────────────────────────────────────────────────────────

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

/**
 * Upper bound on entries kept in memory from live `clipboard.new_content`
 * events. A busy/churning clipboard would otherwise let `items` grow without
 * limit for the whole session. The cap only trims the oldest tail on the
 * live-prepend path; pagination (load-more) may legitimately grow past it and
 * is left untouched. Set far above one page (PAGE_SIZE = 20) so ordinary
 * browsing is never trimmed — combined with list virtualization this bounds
 * both memory and per-update reconcile cost (issue #1129).
 */
const MAX_LIVE_ITEMS = 200

interface ClipboardState {
  items: ClipboardEntry[]
  pendingItems: PendingClipboardEntry[]
  loading: boolean
  notReady: boolean
  error: string | null
  deleteConfirmId: string | null
  staleEntryIds: string[]
}

// 初始状态
const initialState: ClipboardState = {
  items: [],
  pendingItems: [],
  loading: false,
  notReady: false,
  error: null,
  deleteConfirmId: null,
  staleEntryIds: [],
}

// 定义获取剪贴板项目的参数接口
interface FetchClipboardItemsParams {
  orderBy?: OrderBy
  limit?: number
  offset?: number
  isFavorited?: boolean
  filter?: Filter
}

type FetchClipboardItemsResult = {
  status: 'ready' | 'not_ready'
  items: ClipboardEntry[]
  offset: number
}
type FetchClipboardItemsAction = {
  payload: FetchClipboardItemsResult
  type: string
  meta: { arg?: FetchClipboardItemsParams }
}

// 异步 Thunk Actions
export const fetchClipboardItems = createAsyncThunk<
  FetchClipboardItemsResult,
  FetchClipboardItemsParams | undefined
>('clipboard/fetchItems', async (params = {}, { rejectWithValue, dispatch }) => {
  try {
    // Daemon API: GET /clipboard/entries
    // Note: orderBy and filter are not yet supported by the daemon endpoint.

    const { orderBy: _orderBy, filter: _filter, isFavorited: _isFavorited, ...rest } = params
    const result = await getClipboardEntries(rest.limit ?? 50, rest.offset ?? 0)

    // Hydrate durable file transfer statuses from persisted API fields.
    // This ensures entryStatusById in fileTransferSlice is seeded on app load
    // so file entries show correct status badges immediately after restart.
    // fileTransferSlice is the single owner of transfer status; ClipboardEntry
    // deliberately does not carry it.
    if (result.status === 'ready' && result.entries) {
      const statusEntries = result.entries
        .filter(item => item.fileTransferStatus != null)
        .map(item => ({
          entryId: item.id,
          status: item.fileTransferStatus as
            | 'pending'
            | 'transferring'
            | 'completed'
            | 'failed'
            | 'cancelled',
          reason: item.fileTransferReason ?? null,
        }))
      if (statusEntries.length > 0) {
        dispatch(hydrateEntryTransferStatuses(statusEntries))
      }
    }

    if (result.status === 'not_ready') {
      return { status: 'not_ready' as const, items: [], offset: params.offset ?? 0 }
    }
    const items = result.entries?.map(projectClipboardEntry) ?? []
    return { status: 'ready' as const, items, offset: params.offset ?? 0 }
  } catch {
    return rejectWithValue('获取剪贴板内容失败')
  }
})

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

export const toggleFavoriteItem = createAsyncThunk(
  'clipboard/toggleFavorite',
  async ({ id, isFavorited }: { id: string; isFavorited: boolean }, { rejectWithValue }) => {
    try {
      // Daemon API: PUT /clipboard/entries/:id/favorite { is_favorited }
      await toggleFavorite(id, isFavorited)
      return { id, isFavorited }
    } catch {
      return rejectWithValue('设置收藏状态失败')
    }
  }
)

export const clearAllItems = createAsyncThunk(
  'clipboard/clearAll',
  async (_, { rejectWithValue }) => {
    try {
      // Daemon API: POST /clipboard/entries/clear
      // Note: We call the thunk without awaiting its result - the result contains
      // { deletedCount, failedEntries } but we just need success/failure
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

// 创建 Slice
const clipboardSlice = createSlice({
  name: 'clipboard',
  initialState,
  reducers: {
    setDeleteConfirmId: (state, action: PayloadAction<string | null>) => {
      state.deleteConfirmId = action.payload
    },
    setNotReady: (state, action: PayloadAction<boolean>) => {
      state.notReady = action.payload
      if (action.payload) {
        state.loading = false
        state.error = null
      }
    },
    clearError: state => {
      state.error = null
    },
    prependItem: (state, action: PayloadAction<ClipboardEntry>) => {
      if (state.items.some(item => item.id === action.payload.id)) return
      state.items.unshift(action.payload)
      // Bound live growth: drop the oldest tail once the in-memory list
      // outgrows the cap. Older entries remain in the daemon and reappear via
      // pagination/refresh.
      if (state.items.length > MAX_LIVE_ITEMS) {
        state.items.splice(MAX_LIVE_ITEMS)
      }
    },
    removeItem: (state, action: PayloadAction<string>) => {
      state.items = state.items.filter(item => item.id !== action.payload)
    },
    resetItems: state => {
      state.items = []
      state.error = null
    },
    markEntryStale: (state, action: PayloadAction<string>) => {
      if (!state.staleEntryIds.includes(action.payload)) {
        state.staleEntryIds.push(action.payload)
      }
    },
    clearStaleEntries: state => {
      state.staleEntryIds = []
    },
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
     * arrives — the real entry will appear in `items` via the normal API
     * refresh path.
     */
    removePendingEntry: (state, action: PayloadAction<string>) => {
      state.pendingItems = state.pendingItems.filter(p => p.entryId !== action.payload)
    },
  },
  extraReducers: builder => {
    // 处理获取剪贴板内容
    builder.addCase(fetchClipboardItems.pending, state => {
      // Only show loading state when there are no cached items.
      // When items already exist (e.g., navigating back to the page),
      // we fetch in the background without triggering skeleton/loading UI.
      if (state.items.length === 0) {
        state.loading = true
      }
      state.error = null
      state.notReady = false
    })
    builder.addCase(fetchClipboardItems.fulfilled, (state, action: FetchClipboardItemsAction) => {
      state.loading = false
      if (action.payload.status === 'not_ready') {
        state.notReady = true
        return
      }

      state.notReady = false
      const pageOffset = action.payload.offset
      if (pageOffset > 0 && state.items.length > 0) {
        const existingIds = new Set(state.items.map(item => item.id))
        for (const item of action.payload.items) {
          if (!existingIds.has(item.id)) {
            state.items.push(item)
          }
        }
      } else {
        state.items = action.payload.items
      }
    })
    builder.addCase(fetchClipboardItems.rejected, (state, action) => {
      state.loading = false
      state.error = action.payload as string
      state.notReady = false
    })

    // 处理删除剪贴板内容
    builder.addCase(removeClipboardItem.fulfilled, (state, action) => {
      state.items = state.items.filter(item => item.id !== action.payload)
      state.deleteConfirmId = null
    })
    builder.addCase(removeClipboardItem.rejected, (state, action) => {
      state.error = action.payload as string
    })

    // 处理收藏状态切换
    builder.addCase(toggleFavoriteItem.fulfilled, (state, action) => {
      const { id, isFavorited } = action.payload
      const item = state.items.find(item => item.id === id)
      if (item) {
        item.isFavorited = isFavorited
      }
    })
    builder.addCase(toggleFavoriteItem.rejected, (state, action) => {
      state.error = action.payload as string
    })

    // 处理清空剪贴板
    builder.addCase(clearAllItems.fulfilled, state => {
      state.items = []
    })
    builder.addCase(clearAllItems.rejected, (state, action) => {
      state.error = action.payload as string
    })

    // 处理复制到剪贴板
    builder.addCase(copyToClipboard.rejected, (state, action) => {
      state.error = action.payload as string
    })
  },
})

// 导出 Actions
export const {
  setDeleteConfirmId,
  setNotReady,
  clearError,
  prependItem,
  removeItem,
  resetItems,
  markEntryStale,
  clearStaleEntries,
  addPendingEntry,
  removePendingEntry,
} = clipboardSlice.actions

// 导出 Reducer
export default clipboardSlice.reducer
