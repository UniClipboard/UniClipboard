import { useTranslation } from 'react-i18next'
import type {
  HistoryEntryTagSummaryDto,
  HistoryTagColorDto,
  HistoryTagDto,
} from '@/api/daemon/history-tags'
import type { TagSuggestion } from '@/components/history/detail/history-tag-suggestions'
import { tagLabel } from '@/lib/search-tags'
import { Button, Text } from '@/ui-kit'
import { HistoryKitSelectionTag } from './HistoryKitSelectionTag'
import { HistoryKitTagEditor } from './HistoryKitTagEditor'

/** Tag state and actions for the selected rows. */
export interface HistoryKitSelectionTagging {
  tags: HistoryTagDto[]
  /** Tags on the selection; `null` while loading. */
  summary: HistoryEntryTagSummaryDto | null
  editorOpen: boolean
  onEditorOpenChange: (open: boolean) => void
  onAddToAll: (tagId: string) => Promise<boolean>
  onCreateForAll: (name: string, color: HistoryTagColorDto) => Promise<boolean>
  onRemoveFromAll: (tagId: string) => Promise<boolean>
}

/**
 * The selection's tags (design MC2): a tag only some items carry is striped and says how many;
 * clicking it adds it to all, × removes it from all, and the field adds a tag to all of them.
 */
export function HistoryKitSelectionTags({
  count,
  tagging,
}: {
  count: number
  tagging: HistoryKitSelectionTagging
}) {
  const { t } = useTranslation()
  const byId = new Map(tagging.tags.map(tag => [tag.tagId, tag]))
  const nameOf = (tagId: string) =>
    `#${tagLabel({ id: tagId, name: byId.get(tagId)?.name, isBuiltin: false }, t)}`
  const selected = tagging.summary?.selected ?? count
  // Most applied first; ties by name, so the order does not depend on the daemon's.
  const applied = [...(tagging.summary?.tags ?? [])].sort(
    (a, b) => b.applied - a.applied || nameOf(a.tagId).localeCompare(nameOf(b.tagId))
  )
  const onAll = new Set(applied.filter(tag => tag.applied === selected).map(tag => tag.tagId))
  const pick = (suggestion: TagSuggestion, color: HistoryTagColorDto) =>
    suggestion.kind === 'create'
      ? tagging.onCreateForAll(suggestion.name, color)
      : tagging.onAddToAll(suggestion.tag.tagId)
  return (
    <section className="flex shrink-0 flex-col gap-2" aria-label={t('history.tags.selectionTitle')}>
      <Text role="headline">{t('history.tags.title')}</Text>
      <div className="flex flex-wrap items-center gap-1.5">
        {applied.map(({ tagId, applied: on }) => (
          <HistoryKitSelectionTag
            key={tagId}
            tagId={tagId}
            name={nameOf(tagId)}
            on={on}
            selected={selected}
            onAddToAll={() => void tagging.onAddToAll(tagId)}
            onRemoveFromAll={() => void tagging.onRemoveFromAll(tagId)}
          />
        ))}
        {tagging.editorOpen ? (
          <HistoryKitTagEditor
            tags={tagging.tags}
            attachedIds={onAll}
            onPick={pick}
            onClose={() => tagging.onEditorOpenChange(false)}
          />
        ) : (
          <Button size="small" icon="add" onClick={() => tagging.onEditorOpenChange(true)}>
            {t('history.tags.tagAll', { n: selected })}
          </Button>
        )}
      </div>
      {applied.some(tag => tag.applied < selected) && (
        <Text role="caption" tone="secondary">
          {t('history.kit.partialTagHint')}
        </Text>
      )}
    </section>
  )
}
