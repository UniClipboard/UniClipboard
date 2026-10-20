import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { daemonClient } from '@/api/daemon/client'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { HistoryKitDetail } from '@/pages/history-kit/HistoryKitDetail'
import { store } from '@/store'
import { Pane, UiRoot } from '@/ui-kit'
import '@/App.css'
import { installFakeHost } from './fake-host'

void i18n.changeLanguage('zh-CN')

// Production detail and skins; HTTP is controlled by history-detail-regressions.mjs.
installFakeHost({
  GetDaemonSession: () => ({ sessionToken: 'fixture', expiresInSecs: 300, refreshAtSecs: 240 }),
})
daemonClient.initialize({ baseUrl: location.origin, wsUrl: `ws://${location.host}/ws` })
const noop = () => {}
const items: DisplayClipboardItem[] = ['text-a', 'text-b'].map(id => ({
  id,
  type: 'text',
  activeTime: 1_790_000_000_000,
  contentTags: [],
  content: { display_text: `${id} preview`, has_detail: true, size: 12000, char_count: 12000 },
}))
items.push({
  id: 'image',
  type: 'image',
  activeTime: 1_790_000_000_000,
  content: { width: 0, height: 0, size: 1024 },
})

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
