import { createSlice, createAsyncThunk } from '@reduxjs/toolkit'
import { getClipboardStats, type ClipboardStats as DaemonClipboardStats } from '@/api/daemon'
// Tauri API — kept as commented reference during transition

// import { getClipboardStats as getClipboardStatsTauri } from '@/api/clipboardItems'

// Alias to the ClipboardStats name used throughout the slice
type ClipboardStats = DaemonClipboardStats

interface StatsState {
  stats: ClipboardStats
  loading: boolean
  error: string | null
}

const initialState: StatsState = {
  stats: {
    totalItems: 0,
    totalSize: 0,
  },
  loading: false,
  error: null,
}

// 异步 Thunk Actions
export const fetchStats = createAsyncThunk<ClipboardStats, void>(
  'stats/fetchStats',
  async (_, { rejectWithValue }) => {
    try {
      const response = await getClipboardStats()
      return response
    } catch (error) {
      return rejectWithValue(error instanceof Error ? error.message : '未知错误')
    }
  }
)

const statsSlice = createSlice({
  name: 'stats',
  initialState,
  reducers: {},
  extraReducers: builder => {
    builder
      .addCase(fetchStats.pending, state => {
        state.loading = true
        state.error = null
      })
      .addCase(fetchStats.fulfilled, (state, action) => {
        state.loading = false
        state.stats = action.payload
      })
      .addCase(fetchStats.rejected, (state, action) => {
        state.loading = false
        state.error = action.error.message || '获取统计信息失败'
      })
  },
})

// eslint-disable-next-line no-empty-pattern
export const {} = statsSlice.actions
export default statsSlice.reducer
