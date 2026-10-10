import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto, HistoryTagDto } from '@/api/daemon/history-tags'
import { useTagTints } from '@/components/history/tags/tag-colors-context'
import { tagLabel } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { DETAIL_SECTION_LABEL } from './detail-styles'
import type { TagSuggestion } from './history-tag-suggestions'
import HistoryTagEditor from './HistoryTagEditor'

/** What the detail column needs to show and change one entry's local tags. */
export interface DetailTagsProps {
  /** This device's tags (names and counts). */
  tags: HistoryTagDto[]
  editorOpen: boolean
  onEditorOpenChange: (open: boolean) => void
  /** Resolve to whether the change went through. */
  onAdd: (tagId: string) => Promise<boolean>
  onCreate: (name: string, color: HistoryTagColorDto) => Promise<boolean>
  onRemove: (tagId: string) => Promise<boolean>
}

interface HistoryDetailTagsProps extends DetailTagsProps {
  tagIds: string[]
}

/** HDetail.dc.html's TAGS block: the entry's tags as removable chips, then
 * "+ Add tag" (T) or the inline editor. Tags stay in this device's history. */
function HistoryDetailTags({
  tagIds,
  tags,
  editorOpen,
  onEditorOpenChange,
  onAdd,
  onCreate,
  onRemove,
}: HistoryDetailTagsProps) {
  const { t } = useTranslation()
  const tintOf = useTagTints()
  const byId = new Map(tags.map(tag => [tag.tagId, tag]))
  const attached = new Set(tagIds)

  const handlePick = (suggestion: TagSuggestion, color: HistoryTagColorDto) =>
    suggestion.kind === 'create' ? onCreate(suggestion.name, color) : onAdd(suggestion.tag.tagId)

  return (
    <section className="flex shrink-0 flex-col gap-2" aria-label={t('history.tags.title')}>
      <h3 className={DETAIL_SECTION_LABEL}>{t('history.tags.title')}</h3>
      <div className="flex flex-wrap items-center gap-1.5">
        {tagIds.map(id => {
          const label = `#${tagLabel({ id, name: byId.get(id)?.name, isBuiltin: false }, t)}`
          const tint = tintOf(id)
          return (
            <span
              key={id}
              data-testid="detail-tag-chip"
              style={tint.style}
              className={cn(
                'inline-flex h-6.5 items-center gap-1 rounded-full pl-2.5 pr-1 text-ui-caption font-semibold',
                tint.chip
              )}
            >
              {label}
              <button
                type="button"
                aria-label={t('history.tags.remove', { name: label })}
                onClick={() => void onRemove(id)}
                className="inline-flex size-4.5 items-center justify-center rounded-full opacity-70 hover:bg-current/10 hover:opacity-100"
              >
                <X className="size-2.5" strokeWidth={3} aria-hidden="true" />
              </button>
            </span>
          )
        })}
        {editorOpen ? (
          <HistoryTagEditor
            tags={tags}
            attachedIds={attached}
            onPick={handlePick}
            onClose={() => onEditorOpenChange(false)}
          />
        ) : (
          <button
            type="button"
            onClick={() => onEditorOpenChange(true)}
            className="inline-flex h-6.5 items-center gap-1.5 rounded-full border border-dashed border-border bg-background px-2.5 text-ui-caption font-medium text-foreground/80 hover:bg-muted/60"
          >
            + {t('history.tags.add')}
          </button>
        )}
      </div>
    </section>
  )
}

export default HistoryDetailTags
