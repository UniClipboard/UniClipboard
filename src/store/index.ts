import { configureStore } from '@reduxjs/toolkit'
import { createDiagnosticsEnhancer } from '@/observability/diagnostics'
import { appApi } from './api'
import clipboardReducer from './slices/clipboardSlice'
import devicesReducer from './slices/devicesSlice'
import fileTransferReducer from './slices/fileTransferSlice'
import statsReducer from './slices/statsSlice'

const diagnosticsEnhancer = createDiagnosticsEnhancer()

export const store = configureStore({
  reducer: {
    [appApi.reducerPath]: appApi.reducer,
    clipboard: clipboardReducer,
    stats: statsReducer,
    devices: devicesReducer,
    fileTransfer: fileTransferReducer,
  },
  middleware: getDefaultMiddleware => getDefaultMiddleware().concat(appApi.middleware),
  enhancers: getDefaultEnhancers => {
    const enhancers = getDefaultEnhancers()
    return diagnosticsEnhancer ? enhancers.concat(diagnosticsEnhancer) : enhancers
  },
})

// 从 store 本身推断出 RootState 和 AppDispatch 类型
export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
