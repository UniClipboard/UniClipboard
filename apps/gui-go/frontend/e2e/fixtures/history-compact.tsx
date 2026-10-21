import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { daemonClient } from '@/api/daemon/client'
import { useHistoryLayoutTier } from '@/components/history/layout/history-layout'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { HistoryKitDetail } from '@/pages/history-kit/HistoryKitDetail'
import { HistoryKitGrid } from '@/pages/history-kit/HistoryKitGrid'
import { HistoryKitList } from '@/pages/history-kit/HistoryKitList'
import { HistoryKitToolbar, HistoryKitViewSwitch } from '@/pages/history-kit/HistoryKitToolbar'
import { store } from '@/store'
import { Pane, SidebarNav, SplitView, TokenSearchField, UiRoot } from '@/ui-kit'
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
const noop = () => {}
const items: DisplayClipboardItem[] = Array.from({ length: 60 }, (_, index) => ({
  id: `text-${index}`,
  type: 'text',
  activeTime: Date.now() - index * 60_000,
  contentTags: [],
  content: {
    display_text: `记录 ${index}：窄窗口中的剪贴板内容`,
    has_detail: true,
    size: 100,
    char_count: 30,
  },
}))

/** Real skin, list, detail, toolbar and width tiers; synthetic content and native transport. */
export default function Fixture() {
  const drawer = useHistoryLayoutTier() === 'compact'
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [detail, setDetail] = useState(false)
  const [selection, setSelection] = useState(['text-0'])
  const [category, setCategory] = useState('all')
  const [query, setQuery] = useState('')
  const [view, setView] = useState<'list' | 'grid'>('list')
  const [gridSize, setGridSize] = useState(0.4)
  const scheme = new URLSearchParams(location.search).get('scheme') === 'light' ? 'light' : 'dark'
  const selected = items.find(item => item.id === selection.at(-1)) ?? null
  const showDetail = view === 'list' || detail
  const inspect = (id: string) => {
    setSelection([id])
    setDetail(true)
  }
  return (
    <Provider store={store}>
      <ShortcutProvider>
        <div style={{ height: '100vh' }}>
          <UiRoot colorScheme={scheme}>
            <SplitView
              sidebar={
                <SidebarNav
                  label="资料库"
                  sections={[
                    {
                      id: 'library',
                      items: [
                        { id: 'all', label: '全部', icon: 'history' },
                        { id: 'pinned', label: '置顶', icon: 'pin' },
                      ],
                    },
                  ]}
                  value={category}
                  onValueChange={id => {
                    setCategory(id)
                    setDetail(false)
                    if (drawer) setSidebarOpen(false)
                  }}
                />
              }
              sidebarOverlay={drawer}
              sidebarHidden={drawer && !sidebarOpen}
              sidebarToggle={{
                label: drawer && !sidebarOpen ? '显示侧栏' : '隐藏侧栏',
                onClick: () => setSidebarOpen(open => !open),
              }}
              detailNavigation={{ open: detail, onOpenChange: setDetail, backLabel: '返回列表' }}
              title={category === 'all' ? '全部' : '置顶'}
              subtitle="60 项"
              toolbarLeading={
                <HistoryKitViewSwitch
                  view={view}
                  onViewChange={next => {
                    setView(next)
                    setDetail(false)
                  }}
                  gridSize={gridSize}
                  onGridSizeChange={setGridSize}
                />
              }
              search={
                <TokenSearchField
                  label="搜索"
                  placeholder="搜索剪贴板"
                  shortcutHint="Ctrl+F"
                  value={query}
                  onValueChange={setQuery}
                  tokens={[]}
                  suggestions={[]}
                  suggestionsOpen={false}
                  onSuggestionsOpenChange={noop}
                  highlighted={-1}
                  onHighlightedChange={noop}
                  onSelectSuggestion={noop}
                />
              }
              toolbar={
                <HistoryKitToolbar
                  targetIds={selection}
                  pinned={false}
                  onTogglePin={noop}
                  onDelete={noop}
                />
              }
            >
              <Pane kind={showDetail ? 'list' : 'main'} label="历史记录">
                {view === 'grid' ? (
                  <HistoryKitGrid
                    items={items}
                    sourceDeviceNames={{}}
                    size={gridSize}
                    selection={selection}
                    onSelectionChange={setSelection}
                    onOpen={inspect}
                    onDelete={noop}
                    hasMore={false}
                    onLoadMore={noop}
                    contextMenu={{ items: () => [], onAction: noop }}
                  />
                ) : (
                  <HistoryKitList
                    items={items}
                    sourceDeviceNames={{}}
                    selection={selection}
                    onSelectionChange={setSelection}
                    onInspect={inspect}
                    onCopy={noop}
                    onDelete={noop}
                    hasMore={false}
                    loading={false}
                    onLoadMore={noop}
                    contextMenu={{ items: () => [], onAction: noop }}
                  />
                )}
              </Pane>
              {showDetail && (
                <Pane kind="detail" label="详情">
                  <HistoryKitDetail
                    item={selected}
                    tagging={null}
                    imageNeighbors={{ position: 0, total: 0 }}
                    copySuccess={false}
                    onCopy={noop}
                    onTogglePin={noop}
                    onDelete={noop}
                  />
                </Pane>
              )}
            </SplitView>
          </UiRoot>
        </div>
      </ShortcutProvider>
    </Provider>
  )
}

createRoot(document.getElementById('root')!).render(<Fixture />)
