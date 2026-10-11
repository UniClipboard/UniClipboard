import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto, HistoryTagDto } from '@/api/daemon/history-tags'
import type { TagSuggestion } from '@/components/history/detail/history-tag-suggestions'
import HistoryTagEditor from '@/components/history/detail/HistoryTagEditor'
import { tagLabel } from '@/lib/search-tags'
import { Button } from '@/ui-kit'
import { HistoryKitTag } from './HistoryKitTag'

/** What the detail pane needs to show and change one entry's local tags. */
export interface HistoryKitTagging {
  tags: HistoryTagDto[]
  editorOpen: boolean
  onEditorOpenChange: (open: boolean) => void
  onAdd: (tagId: string) => Promise<boolean>
  onCreate: (name: string, color: HistoryTagColorDto) => Promise<boolean>
  onRemove: (tagId: string) => Promise<boolean>
}

/** The entry's tags as removable chips, then "Add Tag" (T) or the tag editor. */
export function HistoryKitDetailTags({
  tagIds,
  tagging,
}: {
  tagIds: string[]
  tagging: HistoryKitTagging
}) {
  const { t } = useTranslation()
  const byId = new Map(tagging.tags.map(tag => [tag.tagId, tag]))
  const pick = (suggestion: TagSuggestion, color: HistoryTagColorDto) =>
    suggestion.kind === 'create'
      ? tagging.onCreate(suggestion.name, color)
      : tagging.onAdd(suggestion.tag.tagId)
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-1.5"
      role="group"
      aria-label={t('history.tags.title')}
    >
      {tagIds.map(id => {
        const name = byId.get(id)?.name
        const label = `#${tagLabel({ id, name, isBuiltin: false }, t)}`
        return (
          <HistoryKitTag
            key={id}
            id={id}
            name={name}
            remove={{
              label: t('history.tags.remove', { name: label }),
              onClick: () => void tagging.onRemove(id),
            }}
          />
        )
      })}
      {tagging.editorOpen ? (
        <HistoryTagEditor
          tags={tagging.tags}
          attachedIds={new Set(tagIds)}
          onPick={pick}
          onClose={() => tagging.onEditorOpenChange(false)}
        />
      ) : (
        <Button
          size="small"
          icon="add"
          tooltip={`${t('history.tags.add')} (T)`}
          onClick={() => tagging.onEditorOpenChange(true)}
        >
          {t('history.tags.add')}
        </Button>
      )}
    </div>
  )
}
