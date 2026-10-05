import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/motion/context-menu'
import { cn } from '@/lib/utils'
import type { LibraryTag } from './history-tag-library'

interface HistoryTagManagerBulkBarProps {
  /** The checked tags, two or more for a merge. */
  checked: LibraryTag[]
  /** Every tag the checked ones can merge into (named ones only). */
  mergeTargets: LibraryTag[]
  onMerge: (targetTagId: string, sourceTagIds: string[]) => Promise<void>
  onDelete: (tagIds: string[]) => Promise<void>
  onClear: () => void
}

const ACTION_CLASS =
  'inline-flex h-8 shrink-0 items-center rounded-lg px-3 text-ui-body font-medium hover:bg-muted disabled:opacity-50'

/**
 * The tag manager's footer while tags are checked (HManage.dc.html "Select
 * several to merge or delete at once"): merge them all into one tag, or
 * delete them all; their items stay either way.
 */
function HistoryTagManagerBulkBar({
  checked,
  mergeTargets,
  onMerge,
  onDelete,
  onClear,
}: HistoryTagManagerBulkBarProps) {
  const { t } = useTranslation()
  const ids = checked.map(tag => tag.tagId)
  // A target among the checked tags absorbs the others; one outside them
  // absorbs all of them. Merging one tag into itself is no merge.
  const targets = mergeTargets.filter(target => !(ids.length === 1 && ids[0] === target.tagId))

  return (
    <div
      role="toolbar"
      aria-label={t('history.tags.bulkActions')}
      className="flex h-12 shrink-0 items-center gap-1 border-t border-border/60 bg-muted/40 pl-6 pr-4"
    >
      <span className="flex-1 text-ui-body font-semibold">
        {t('history.tags.checkedCount', { n: checked.length })}
      </span>
      <ContextMenu>
        <ContextMenuTrigger activation="click">
          <button type="button" className={ACTION_CLASS} disabled={targets.length === 0}>
            {t('history.tags.mergeInto')}
          </button>
        </ContextMenuTrigger>
        <ContextMenuContent
          side="top"
          ariaLabel={t('history.tags.mergeInto')}
          className="max-h-72 w-52 overflow-y-auto"
        >
          {targets.map(target => (
            <ContextMenuItem
              key={target.tagId}
              textValue={target.name ?? target.tagId}
              onSelect={() =>
                void onMerge(
                  target.tagId,
                  ids.filter(id => id !== target.tagId)
                )
              }
            >
              <span className="truncate">#{target.name}</span>
            </ContextMenuItem>
          ))}
        </ContextMenuContent>
      </ContextMenu>
      <button
        type="button"
        className={cn(ACTION_CLASS, 'font-semibold text-orange-700 dark:text-orange-300')}
        onClick={() => void onDelete(ids)}
      >
        {t('history.tags.deleteChecked', { n: checked.length })}
      </button>
      <button type="button" className={ACTION_CLASS} onClick={onClear}>
        {t('history.tags.cancel')}
      </button>
    </div>
  )
}

export default HistoryTagManagerBulkBar
