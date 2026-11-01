import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { daemonClient } from '@/api/daemon/client'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { HistoryKitDetail } from '@/pages/history-kit/HistoryKitDetail'
import { store } from '@/store'
import { addPendingEntry } from '@/store/slices/clipboardSlice'
import { updateTransferProgress } from '@/store/slices/fileTransferSlice'
import { replaceOutboundSnapshots } from '@/store/slices/outboundProgressSlice'
import { Pane, UiRoot } from '@/ui-kit'
import '@/App.css'
import { installFakeHost } from './fake-host'

void i18n.changeLanguage('zh-CN')

// Production detail and skins; HTTP is controlled by tester-army/file-preview.e2e.mjs.
installFakeHost({
  GetDaemonSession: () => ({ sessionToken: 'fixture', expiresInSecs: 300, refreshAtSecs: 240 }),
})
daemonClient.initialize({ baseUrl: location.origin, wsUrl: `ws://${location.host}/ws` })
const noop = () => {}
const examples = {
  single: ['设计说明.pdf'],
  image: ['风景.png'],
  mixed: [
    '33608dbdbcd090c5565314-long-archive-name.zip',
    'photo_1783744743419.jpg',
    'photo_另一张.jpg',
    '说明.pdf',
  ],
  gallery: ['风景.png', '建筑.png', '森林.png'],
  failed: ['已接收.pdf', '文档.zip', '照片.jpg', '说明.txt'],
  active: ['文档.zip', '照片.jpg'],
  local: ['本地资料.pdf'],
  'receive-files': ['stale-name-A.zip', 'stale-name-B.bin', 'stale-name-C.pdf'],
  'receive-race': ['stale-name-A.zip', 'stale-name-B.bin', 'stale-name-C.pdf'],
  'receive-saving': ['素材.zip', '文档.pdf'],
  'receive-unknown': ['历史文件.zip'],
  'receive-zero': ['空文件.txt'],
  'receive-directory': ['目录 A', '目录 B'],
  'outbound-single': ['发布素材.zip'],
  'outbound-multi': ['发布素材.zip', '产品演示.mov', '设计说明.pdf'],
  'outbound-unknown': ['空文件.txt', '未知大小.bin'],
  'outbound-finished': ['发布素材.zip', '产品演示.mov', '设计说明.pdf'],
}
const items: DisplayClipboardItem[] = Object.entries(examples).map(([id, names]) => ({
  id,
  type: 'file',
  activeTime: 1_790_000_000_000,
  content: {
    file_names: names,
    file_sizes: names.map(() => 2048),
    file_paths: names.map(name => `/fixture/${name}`),
  },
}))

// Failure modes: target counters get merged, indices cross file/representation namespaces,
// an unknown total becomes zero, 100% becomes success, terminal errors disappear,
// or one platform skin clips the file progress. These are fixture-backed browser flows.
function outboundFor(id: string): OutboundProgressSnapshot[] {
  if (!id.startsWith('outbound-')) return []
  const names = examples[id as keyof typeof examples]
  return Array.from(
    { length: id === 'outbound-single' || id === 'outbound-unknown' ? 1 : 3 },
    (_, device) => ({
      entryId: id,
      targetDeviceId: ['macbook', 'work-pc', 'pixel'][device],
      revision: 1,
      freshness: 'live',
      freshnessAvailable: true,
      state: 'in_progress',
      itemsTotal: names.length,
      itemsCompleted: 0,
      itemsCancelled: 0,
      itemsFailed: 0,
      itemsRunning: names.length,
      itemsUnconfirmed: 0,
      itemsWaiting: 0,
      items: names.map((_, index) => ({
        kind: 'file',
        index,
        state:
          id === 'outbound-finished'
            ? device === 1 && index === 1
              ? 'failed'
              : 'completed'
            : id === 'outbound-unknown'
              ? index === 0
                ? 'completed'
                : 'unconfirmed'
              : index === 2
                ? 'processing'
                : index === 1
                  ? ['waiting', 'failed', 'cancelled'][device]
                  : device === 2
                    ? 'completed'
                    : 'transferring',
        completedBytes:
          id === 'outbound-unknown'
            ? index === 0
              ? 0
              : null
            : index === 2 || id === 'outbound-finished' || (device === 2 && index === 0)
              ? 2048
              : index === 1 && device === 0
                ? null
                : device === 0
                  ? 1024
                  : 512,
        totalBytes:
          id === 'outbound-unknown'
            ? index === 0
              ? 0
              : null
            : index === 1 && device === 0
              ? null
              : 2048,
        failureReason:
          (id === 'outbound-finished' || id === 'outbound-multi') && index === 1 && device === 1
            ? 'network_unavailable'
            : null,
        cancellationReason:
          id === 'outbound-multi' && index === 1 && device === 2 ? 'local_user' : null,
      })),
    })
  )
}

store.dispatch(
  updateTransferProgress({
    transferId: 'active-transfer',
    entryId: 'active',
    attemptId: 'active-attempt',
    peerId: 'peer',
    direction: 'receiving',
    bytesTransferred: 1024,
    totalBytes: 4096,
  })
)

export default function Fixture() {
  const [index, setIndex] = useState(0)
  const [revision, setRevision] = useState(0)
  const scheme = new URLSearchParams(location.search).get('scheme') === 'light' ? 'light' : 'dark'
  return (
    <Provider store={store}>
      <ShortcutProvider>
        <UiRoot colorScheme={scheme}>
          <div style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}>
            <nav
              style={{
                display: 'flex',
                flexShrink: 0,
                gap: '1rem',
                padding: '1rem',
                flexWrap: 'wrap',
              }}
            >
              {items.map((item, i) => (
                <button
                  type="button"
                  key={item.id}
                  onClick={() => {
                    setIndex(i)
                    if (item.id === 'receive-saving')
                      store.dispatch(
                        addPendingEntry({
                          entryId: item.id,
                          attemptId: `${item.id}-attempt`,
                          fromDevice: 'peer',
                          totalBytes: null,
                          filenames: examples['receive-saving'],
                          createdAt: Date.now(),
                        })
                      )
                    store.dispatch(replaceOutboundSnapshots(outboundFor(item.id)))
                  }}
                >
                  {item.id}
                </button>
              ))}
              <button
                type="button"
                onClick={() => document.dispatchEvent(new Event('visibilitychange'))}
              >
                refresh receive
              </button>
              <button type="button" onClick={() => setRevision(revision + 1)}>
                refresh item
              </button>
            </nav>
            <Pane kind="detail" label="Detail">
              <HistoryKitDetail
                item={revision ? { ...items[index] } : items[index]}
                tagging={null}
                imageNeighbors={{ position: 1, total: 1 }}
                copySuccess={false}
                onCopy={noop}
                onTogglePin={noop}
                onDelete={noop}
              />
            </Pane>
          </div>
        </UiRoot>
      </ShortcutProvider>
    </Provider>
  )
}
createRoot(document.getElementById('root')!).render(<Fixture />)
