import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { daemonClient } from '@/api/daemon/client'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { HistoryKitDetail } from '@/pages/history-kit/HistoryKitDetail'
import { store } from '@/store'
import { updateTransferProgress } from '@/store/slices/fileTransferSlice'
import { Pane, UiRoot } from '@/ui-kit'
import '@/App.css'
import { installFakeHost } from './fake-host'

void i18n.changeLanguage('zh-CN')

// Production detail and skins; HTTP is controlled by file-preview-browser.mjs.
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
            <nav style={{ display: 'flex', flexShrink: 0, gap: '1rem', padding: '1rem' }}>
              {items.map((item, i) => (
                <button type="button" key={item.id} onClick={() => setIndex(i)}>
                  {item.id}
                </button>
              ))}
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
