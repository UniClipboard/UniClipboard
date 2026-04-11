import { configureStore } from '@reduxjs/toolkit'
import { appApi } from './api'
import clipboardReducer from './slices/clipboardSlice'
import devicesReducer from './slices/devicesSlice'
import fileTransferReducer from './slices/fileTransferSlice'
import statsReducer from './slices/statsSlice'
import { redactSensitiveArgs } from '@/observability/redaction'
import { Sentry, sentryEnabled } from '@/observability/sentry'

const sentryReduxEnhancer = sentryEnabled
  ? Sentry.createReduxEnhancer({
      stateTransformer: state => redactSensitiveArgs(state) as Record<string, unknown>,
    })
  : undefined

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
    return sentryReduxEnhancer ? enhancers.concat(sentryReduxEnhancer) : enhancers
  },
})

// 从 store 本身推断出 RootState 和 AppDispatch 类型
export type RootState = ReturnType<typeof store.getState>
export type AppDispatch = typeof store.dispatch
