import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import type { HistoryTagDto } from '@/api/daemon/history-tags'
import { TagColorsContext } from '@/components/history/tags/tag-colors-context'
import i18n from '@/i18n'
import { HistoryKitDetailTags } from '@/pages/history-kit/HistoryKitDetailTags'
import { UiRoot } from '@/ui-kit'

// Isolated rendering of the production detail tag controls; not daemon or native proof.
export function TagMenuFixture() {
  const [open, setOpen] = useState(false)
  const [ids, setIds] = useState<string[]>([])
  const tags: HistoryTagDto[] = new URLSearchParams(location.search).has('empty')
    ? []
    : [
        { tagId: 'release', name: 'release', entryCount: 12, createdAtMs: 0 },
        { tagId: 'hot-path', name: 'hot-path', entryCount: 3, createdAtMs: 0 },
      ]
  return (
    <I18nextProvider i18n={i18n}>
      <TagColorsContext value={{ release: 'orange', 'hot-path': 'blue' }}>
        <UiRoot
          colorScheme={
            new URLSearchParams(location.search).get('theme') === 'dark' ? 'dark' : 'light'
          }
        >
          <div
            style={{
              height: '100vh',
              display: 'flex',
              justifyContent: 'flex-end',
              padding: '8rem 2rem',
            }}
          >
            <section
              aria-label="Item detail"
              style={{ width: '20rem', height: '18rem', overflow: 'auto' }}
            >
              <HistoryKitDetailTags
                tagIds={ids}
                tagging={{
                  tags,
                  editorOpen: open,
                  onEditorOpenChange: setOpen,
                  onAdd: async id => {
                    setIds([...ids, id])
                    return true
                  },
                  onCreate: async (name, color) => {
                    Object.assign(window, { __tagMenuCreated: { name, color } })
                    return true
                  },
                  onRemove: async id => {
                    setIds(ids.filter(v => v !== id))
                    return true
                  },
                }}
              />
              <button type="button" style={{ marginTop: '15rem' }}>
                Outside target
              </button>
            </section>
          </div>
        </UiRoot>
      </TagColorsContext>
    </I18nextProvider>
  )
}
void i18n
  .changeLanguage('en-US')
  .then(() => createRoot(document.getElementById('root')!).render(<TagMenuFixture />))
