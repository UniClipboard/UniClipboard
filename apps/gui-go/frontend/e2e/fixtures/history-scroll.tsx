import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { GroupedGrid, GroupedList, UiRoot } from '@/ui-kit'
import '@/App.css'

const params = new URLSearchParams(location.search)
const items = Array.from({ length: 2000 }, (_, index) => ({
  id: `entry-${index}`,
  title: `Clipboard entry ${index}: synthetic history for scroll measurements`,
  subtitle: 'Test device · 12:30',
  kind: 'text' as const,
  text: 'Synthetic clipboard preview. '.repeat(7),
}))
const groups = Array.from({ length: 20 }, (_, index) => ({
  id: `day-${index}`,
  title: `Day ${index}`,
  count: 100,
}))
document.documentElement.dataset.ucLowEffects = 'true'
document.documentElement.dataset.ucReduceMotion = 'true'

/** Synthetic history only; never connects to a daemon or native host. */
export default function Fixture() {
  const [selection, setSelection] = useState<string[]>([])
  const props = { label: 'History', items, groups, selection, onSelectionChange: setSelection }
  return (
    <div style={{ height: '100vh' }}>
      <UiRoot colorScheme={params.get('scheme') === 'light' ? 'light' : 'dark'}>
        <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
          {params.has('static') ? (
            <div className="adw-glist">
              <div className="adw-glist-scroller" style={{ overflow: 'auto' }}>
                {items.map(item => (
                  <div key={item.id} className="adw-glist-cell">
                    <div className="adw-row" data-item-id={item.id}>
                      <span className="adw-row-text">
                        <span className="adw-row-title">{item.title}</span>
                        <span className="adw-row-meta">{item.subtitle}</span>
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : params.get('view') === 'grid' ? (
            <GroupedGrid {...props} size={0.4} />
          ) : (
            <GroupedList {...props} />
          )}
        </div>
      </UiRoot>
    </div>
  )
}
createRoot(document.getElementById('root')!).render(<Fixture />)
