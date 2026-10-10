import type React from 'react'
import { useTranslation } from 'react-i18next'
import type {
  HistoryEntryTagSummaryDto,
  HistoryTagColorDto,
  HistoryTagDto,
} from '@/api/daemon/history-tags'
import { useTagTints } from '@/components/history/tags/tag-colors-context'
import { tagLabel } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { DETAIL_SECTION_LABEL } from './detail-styles'
import type { TagSuggestion } from './history-tag-suggestions'
import HistoryTagEditor from './HistoryTagEditor'

/** Local-tag state and actions for the checked rows. */
export interface SelectionTaggingProps {
  tags: HistoryTagDto[]
  /** Tags on the selection; `null` while loading. */
  summary: HistoryEntryTagSummaryDto | null
  editorOpen: boolean
  onEditorOpenChange: (open: boolean) => void
  onAddToAll: (tagId: string) => Promise<boolean>
  onCreateForAll: (name: string, color: HistoryTagColorDto) => Promise<boolean>
  onRemoveFromAll: (tagId: string) => Promise<boolean>
}

interface HistorySelectionPanelProps {
  count: number
  /** `null` when tags are unavailable: only the count shows. */
  tagging: SelectionTaggingProps | null
}

/**
 * The detail column while several rows are checked (HDetail.dc.html `multi`):
 * the count and the selection's tags. A partial tag (on some rows) adds to all
 * on click; ⌥-click takes any tag off all of them.
 */
function HistorySelectionPanel({ count, tagging }: HistorySelectionPanelProps) {
  const { t } = useTranslation()
  const tintOf = useTagTints()
  const byId = new Map((tagging?.tags ?? []).map(tag => [tag.tagId, tag]))
  const nameOf = (tagId: string) => byId.get(tagId)?.name ?? tagId
  // Most applied first; ties by name, so the order does not depend on the daemon's.
  const applied = [...(tagging?.summary?.tags ?? [])].sort(
    (a, b) => b.applied - a.applied || nameOf(a.tagId).localeCompare(nameOf(b.tagId))
  )
  const selected = tagging?.summary?.selected ?? count
  const onAll = new Set(applied.filter(tag => tag.applied === selected).map(tag => tag.tagId))

  const handleChip = (event: React.MouseEvent, tagId: string, partial: boolean) => {
    if (!tagging) return
    if (event.altKey) void tagging.onRemoveFromAll(tagId)
    else if (partial) void tagging.onAddToAll(tagId)
  }
  const handlePick = (suggestion: TagSuggestion, color: HistoryTagColorDto) =>
    !tagging
      ? Promise.resolve(false)
      : suggestion.kind === 'create'
        ? tagging.onCreateForAll(suggestion.name, color)
        : tagging.onAddToAll(suggestion.tag.tagId)

  return (
    <section
      aria-label={t('history.detail.aria')}
      className="flex h-full min-w-0 flex-col bg-muted/20"
      data-testid="selection-detail"
    >
      <header className="flex h-15 shrink-0 items-center px-6">
        <span className="text-ui-section">{t('history.list.selected', { count })}</span>
      </header>
      {tagging && (
        <section
          className="flex flex-col gap-2 px-6 py-5"
          aria-label={t('history.tags.selectionTitle')}
        >
          <h3 className={DETAIL_SECTION_LABEL}>{t('history.tags.selectionTitle')}</h3>
          <div className="flex flex-wrap items-center gap-1.5">
            {applied.map(({ tagId, applied: on }) => {
              const name = `#${tagLabel({ id: tagId, name: byId.get(tagId)?.name, isBuiltin: false }, t)}`
              const partial = on < selected
              const tint = tintOf(tagId)
              return (
                <button
                  key={tagId}
                  type="button"
                  data-testid="selection-tag-chip"
                  aria-label={
                    partial
                      ? t('history.tags.addToAll', { name, n: selected })
                      : t('history.tags.onAll', { name, n: selected })
                  }
                  title={t('history.tags.removeFromAllHint')}
                  onClick={event => handleChip(event, tagId, partial)}
                  style={tint.style}
                  className={cn(
                    'inline-flex h-6.5 items-center gap-1.5 rounded-full px-2.5 text-ui-caption font-semibold',
                    partial ? cn('border border-dashed border-current/40', tint.text) : tint.chip
                  )}
                >
                  {name}
                  <span className="font-medium opacity-75">
                    {on}/{selected}
                  </span>
                </button>
              )
            })}
            {tagging.editorOpen ? (
              <HistoryTagEditor
                tags={tagging.tags}
                attachedIds={onAll}
                onPick={handlePick}
                onClose={() => tagging.onEditorOpenChange(false)}
              />
            ) : (
              <button
                type="button"
                onClick={() => tagging.onEditorOpenChange(true)}
                className="inline-flex h-6.5 items-center gap-1.5 rounded-full border border-dashed border-border bg-background px-2.5 text-ui-caption font-medium text-foreground/80 hover:bg-muted/60"
              >
                + {t('history.tags.tagAll', { n: selected })}
                <kbd className="font-mono text-muted-foreground">T</kbd>
              </button>
            )}
          </div>
          <p className="text-ui-caption text-muted-foreground">{t('history.tags.selectionHint')}</p>
        </section>
      )}
    </section>
  )
}

export default HistorySelectionPanel
