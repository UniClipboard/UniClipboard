/** Real daemon/SDK/cache check. Only native session bootstrap uses the supplied real JWT. */
import { mock } from 'bun:test'
import { createInterface } from 'node:readline'
import { configureStore } from '@reduxjs/toolkit'

const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]()
const first = await lines.next()
if (first.done) throw new Error('Missing real daemon connection')
const connection = JSON.parse(first.value) as { endpoint: string; session: string }
mock.module('@/lib/ipc', () => ({
  commands: {
    getDaemonSession: async () => ({ sessionToken: connection.session, expiresInSecs: 300 }),
  },
}))
const { daemonClient } = await import('../../../apps/gui-go/frontend/src/api/daemon/client')
const { appApi } = await import('../../../apps/gui-go/frontend/src/store/api')
const store = configureStore({
  reducer: { [appApi.reducerPath]: appApi.reducer },
  middleware: defaults => defaults().concat(appApi.middleware),
})
daemonClient.initialize({
  baseUrl: connection.endpoint,
  wsUrl: connection.endpoint.replace('http:', 'ws:') + '/ws',
})
const report = (value: unknown) =>
  process.stdout.write('CACHE_RECEIPT ' + JSON.stringify(value) + '\n')
report({ ready: true })
try {
  for (;;) {
    const command = await lines.next()
    if (command.done || command.value === 'stop') break
    const request = store.dispatch(
      appApi.endpoints.getConfigImportStatus.initiate(undefined, { forceRefetch: true })
    )
    await request
    const state = appApi.endpoints.getConfigImportStatus.select()(store.getState())
    report({
      isError: state.isError,
      isSuccess: state.isSuccess,
      rejected: state.data?.rejectedImport?.reason ?? null,
    })
    request.unsubscribe()
  }
} finally {
  daemonClient.destroy()
  store.dispatch(appApi.util.resetApiState())
}
process.exit(0)
