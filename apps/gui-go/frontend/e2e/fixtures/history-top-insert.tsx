import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { GroupedGrid, GroupedList, UiRoot } from '@/ui-kit'
import '@/App.css'

const initialItems = Array.from({ length: 40 }, (_, index) => ({
  id: `entry-${index}`,
  title: `Synthetic entry ${index}`,
  subtitle: 'Synthetic device',
  kind: 'text' as const,
  text: `Synthetic entry ${index}`,
}))

/** Exercise the real bounded history components without a daemon or user data. */
export default function Fixture() {
  const [items, setItems] = useState(initialItems)
  const [selection, setSelection] = useState<string[]>([])
  const params = new URLSearchParams(location.search)
  const props = {
    label: 'History',
    items,
    groups: [{ id: 'today', title: 'Today', count: items.length }],
    selection,
    onSelectionChange: setSelection,
    viewport: {
      offset: 0,
      total: items.length,
      onVisibleRange: () => {},
      onNavigate: () => {},
    },
  }
  return (
    <div style={{ height: '100vh' }}>
      <UiRoot colorScheme={params.get('scheme') === 'dark' ? 'dark' : 'light'}>
        <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
          <button
            type="button"
            onClick={() =>
              setItems(previous => [
                { ...initialItems[0], id: `new-${previous.length}`, title: 'New entry' },
                ...previous,
              ])
            }
          >
            Insert entry
          </button>
          {params.get('view') === 'grid' ? (
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
