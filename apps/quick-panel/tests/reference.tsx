import { LazyMotion, domMax } from 'framer-motion'
import React, { useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import { Filter } from '@/api/clipboardItems'
import '@/i18n'
import i18n from '@/i18n'
import '@/styles/globals.css'
import { defaultSearchTagOptions } from '@/lib/search-tags'
import { applyThemePreset } from '@/lib/theme-engine'
import ClipboardPreviewPane from '@/quick-panel/ClipboardPreviewPane'
import HistoryPane from '@/quick-panel/components/HistoryPane'
import { store } from '@/store'

await i18n.changeLanguage('zh-CN')
applyThemePreset('zinc', 'light', document.documentElement)
const previews = [
  'GPUI quick panel paste verification',
  '中文搜索验证：剪贴板历史',
  ...Array.from(
    { length: 48 },
    (_, i) =>
      `Clipboard sample ${i + 2} - a long preview that remains on one line even when the window is narrow`
  ),
]
const items = previews.map((preview, index) => ({
  id: `fixture-${index}`,
  type: 'text' as const,
  preview,
  activeTime: Date.now() - index * 60000,
  isUnavailable: index === 2,
}))
const rich = items.map(item => ({
  ...item,
  content: {
    display_text: item.preview,
    has_detail: false,
    size: item.preview.length,
    char_count: item.preview.length,
  },
  isFavorited: false,
  contentTags: [],
}))
function Reference() {
  const [selected, setSelected] = useState(0)
  const [filter, setFilter] = useState(Filter.All)
  const [query, setQuery] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const refs = useRef(new Map())
  const noop = () => {}
  const visible =
    filter === Filter.All || filter === Filter.Text
      ? items.filter(item => item.preview.toLowerCase().includes(query.toLowerCase()))
      : []
  return (
    <div style={{ width: 760, height: 452, display: 'flex', padding: 16, gap: 8 }}>
      <div style={{ width: 360, height: 420, flexShrink: 0 }}>
        <HistoryPane
          filteredItems={visible}
          interaction={{
            hasPointerMovedSinceShow: true,
            isKeyboardNav: false,
            isLocked: false,
            selectedIndex: selected,
          }}
          isSearching={false}
          searchTotal={125}
          itemRefs={refs.current}
          loading={false}
          onHover={setSelected}
          onHistoryMouseMove={noop}
          onSearchChange={setQuery}
          onSelect={setSelected}
          onContextMenuSelect={setSelected}
          onUnlock={noop}
          searchInputRef={input}
          setHoveredIndex={noop}
          unlocking={false}
          unlockError={null}
          activeFilter={filter}
          setActiveFilter={setFilter}
          tagFilter={null}
          setTagFilter={noop}
          sourceFilter={null}
          setSourceFilter={noop}
          extensionFilter={null}
          setExtensionFilter={noop}
          timeRange="all_time"
          setTimeRange={noop}
          searchableTags={defaultSearchTagOptions()}
          sourceOptions={[]}
          onKeyDown={noop}
          contextItems={rich}
          contextActions={{
            onCopy: noop,
            onPasteFilePaths: noop,
            onToggleFavorite: noop,
            onDelete: noop,
          }}
        />
      </div>
      <div style={{ width: 360, height: 420 }}>
        <ClipboardPreviewPane item={rich[selected]} />
      </div>
    </div>
  )
}
createRoot(document.getElementById('root')!).render(
  <Provider store={store}>
    <LazyMotion features={domMax}>
      <Reference />
    </LazyMotion>
  </Provider>
)
