import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { daemonClient } from '@/api/daemon/client'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import ResendConfirmDialog from '@/components/clipboard/ResendConfirmDialog'
import i18n from '@/i18n'
import { HistoryKitDelivery } from '@/pages/history-kit/HistoryKitDelivery'
import { store } from '@/store'
import { replaceOutboundSnapshots } from '@/store/slices/outboundProgressSlice'
import { UiRoot } from '@/ui-kit'
import '@/App.css'
import { installFakeHost } from './fake-host'

void i18n.changeLanguage('zh-CN')
document.documentElement.classList.toggle(
  'dark',
  new URLSearchParams(location.search).get('scheme') !== 'light'
)
installFakeHost({
  GetDaemonSession: () => ({ sessionToken: 'fixture', expiresInSecs: 300, refreshAtSecs: 240 }),
})
daemonClient.initialize({ baseUrl: location.origin, wsUrl: `ws://${location.host}/ws` })
function update(scenario: string) {
  const state = ['completed', 'stale-completed', 'zero'].includes(scenario)
    ? 'completed'
    : scenario.includes('failed')
      ? 'partially_failed'
      : 'in_progress'
  const items = Array.from({ length: 12 }, (_, index) => ({
    index,
    kind: 'file',
    state:
      state === 'completed' || (scenario.includes('failed') && index > 0)
        ? 'completed'
        : scenario.includes('failed') && index === 0
          ? 'failed'
          : 'transferring',
    completedBytes:
      scenario === 'zero'
        ? 0
        : scenario === 'bytes-complete' ||
            state === 'completed' ||
            (scenario.includes('failed') && index > 0)
          ? 1024
          : 256,
    totalBytes: scenario === 'unknown' ? null : scenario === 'zero' ? 0 : 1024,
    failureReason: scenario.includes('failed') && index === 0 ? 'network_unavailable' : null,
  }))
  const snapshot: OutboundProgressSnapshot = {
    entryId: 'delivery-entry',
    targetDeviceId: 'phone',
    attemptId: 'attempt',
    revision: 1,
    state,
    freshness: scenario.startsWith('stale')
      ? 'unconfirmed'
      : state === 'in_progress'
        ? 'live'
        : 'settled',
    freshnessAvailable: true,
    itemsTotal: 12,
    itemsCompleted: state === 'completed' ? 12 : scenario.includes('failed') ? 11 : 0,
    itemsFailed: scenario.includes('failed') ? 1 : 0,
    itemsCancelled: 0,
    itemsRunning: state === 'in_progress' ? 12 : 0,
    itemsUnconfirmed: 0,
    itemsWaiting: 0,
    items,
  }
  store.dispatch(replaceOutboundSnapshots(scenario === 'no-snapshot' ? [] : [snapshot]))
}
update('active')
export default function Fixture() {
  const [scenario, setScenario] = useState('active')
  const scheme = new URLSearchParams(location.search).get('scheme') === 'light' ? 'light' : 'dark'
  return (
    <Provider store={store}>
      <UiRoot colorScheme={scheme}>
        <main style={{ minHeight: '100vh' }} className="mx-auto flex max-w-xl flex-col gap-8 p-6">
          <nav className="flex flex-wrap gap-2">
            {[
              'active',
              'bytes-complete',
              'completed',
              'unknown',
              'stale',
              'stale-completed',
              'zero',
              'failed',
              'no-snapshot',
              'remote-failed',
            ].map(value => (
              <button
                type="button"
                key={value}
                onClick={() => {
                  setScenario(value)
                  update(value)
                }}
              >
                {value}
              </button>
            ))}
          </nav>
          <HistoryKitDelivery
            fileNames={Array.from({ length: 12 }, (_, index) =>
              index === 0
                ? 'project-assets-2026-final-delivery-package-with-a-long-name.zip'
                : `transfer-file-${index + 1}.bin`
            )}
            delivery={{
              entryId: 'delivery-entry',
              source:
                scenario === 'remote-failed'
                  ? { tag: 'remote', deviceId: 'phone', deviceName: 'iPhone 16 Pro' }
                  : { tag: 'local' },
              deliveries: [
                {
                  targetDeviceId: 'phone',
                  targetDeviceName: 'iPhone 16 Pro',
                  status: { tag: 'delivered' },
                  reasonDetail: null,
                  updatedAtMs: 1,
                },
                {
                  targetDeviceId: 'laptop',
                  targetDeviceName: 'MacBook Pro',
                  status: { tag: 'pending' },
                  reasonDetail: null,
                  updatedAtMs: null,
                },
              ],
            }}
          />
          <ResendConfirmDialog />
        </main>
      </UiRoot>
    </Provider>
  )
}
createRoot(document.getElementById('root')!).render(<Fixture />)
