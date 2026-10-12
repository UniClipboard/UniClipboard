import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Pane, SplitView, Text, UiRoot } from '@/ui-kit'

// Production SplitView with synthetic content; no daemon or user data.
export function SidebarAnimationFixture() {
  const [hidden, setHidden] = useState(false)
  return (
    <UiRoot colorScheme="light">
      <SplitView
        title="Sidebar animation"
        sidebarHidden={hidden}
        sidebarToggle={{
          label: hidden ? 'Show sidebar' : 'Hide sidebar',
          onClick: () => setHidden(value => !value),
        }}
        sidebar={<button type="button">Synthetic sidebar item</button>}
      >
        <Pane kind="list" label="Synthetic list">
          <Text>List</Text>
        </Pane>
        <Pane kind="detail" label="Synthetic detail">
          <Text>Detail</Text>
        </Pane>
      </SplitView>
    </UiRoot>
  )
}
createRoot(document.getElementById('root')!).render(<SidebarAnimationFixture />)
