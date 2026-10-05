import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import HistoryTagManager from '../HistoryTagManager'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

const ok = vi.fn().mockResolvedValue(true)

describe('HistoryTagManager', () => {
  it('lists the user’s tags first and the builtin tags after them, however used', async () => {
    render(
      <ShortcutProvider>
        <HistoryTagManager
          open
          onOpenChange={vi.fn()}
          tags={[
            {
              tagId: '00000000-0000-4000-8000-000000000001',
              name: 'deploy',
              entryCount: 3,
              createdAtMs: 0,
            },
            {
              tagId: '00000000-0000-4000-8000-000000000002',
              name: 'work',
              entryCount: 9,
              createdAtMs: 0,
            },
          ]}
          searchTags={[{ id: 'code', count: 500, isBuiltin: true }]}
          sidebarTagIds={[]}
          onCreate={ok}
          onSetColor={ok}
          onSetInSidebar={ok}
          onRename={vi.fn()}
          onMerge={ok}
          onDelete={ok}
          onShowItems={vi.fn()}
        />
      </ShortcutProvider>
    )

    const rows = await screen.findAllByTestId('tag-manager-row')
    const names = rows.map(row => within(row).getByText(/^#/).textContent)
    expect(names).toEqual([
      '#work',
      '#deploy',
      '#history.type.link',
      '#history.type.code',
      '#history.type.image',
      '#history.type.directory',
    ])
  })
})
