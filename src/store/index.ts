import { configureStore } from '@reduxjs/toolkit'
import { appApi } from './api'
import clipboardReducer from './slices/clipboardSlice'
import devicesReducer from './slices/devicesSlice'
import statsReducer from './slices/statsSlice'

export const store = configureStore({
  reducer: {
    [appApi.reducerPath]: appApi.reducer,
    clipboard: clipboardReducer,
    stats: statsReducer,
    devices: devicesReducer,
  },
  middleware: getDefaultMiddleware => getDefaultMiddleware().concat(appApi.middleware),
})

// 从 store 本身推断出 RootState 和 AppDispatch 类型
export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
