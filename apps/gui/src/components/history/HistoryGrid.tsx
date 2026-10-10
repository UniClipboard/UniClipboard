import { Loader2, Search } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Virtuoso, type StateSnapshot, type VirtuosoHandle } from 'react-virtuoso'
import { deviceLabel } from '@/components/clipboard/entry-delivery-labels'
import { HistoryScroller, HistoryList } from '@/components/history/history-scroll-components'
import HistoryGridRow from '@/components/history/HistoryGridRow'
import { dayKey } from '@/components/history/list/history-list-format'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'

const historyScrollComponents = {
  Scroller: HistoryScroller,
  List: HistoryList,
}

interface HistoryGridProps {
  items: DisplayClipboardItem[]
  /** Origin device id -> name, for the rows' meta line. */
  sourceDeviceNames?: Record<string, string>
  /** Local tag id -> name, for the rows' tag chips; absent while
   * local tags are unavailable. */
  tagNames?: ReadonlyMap<string, string | null>
  /** The bulk-selected ids and their toggle. */
  checkedIds?: ReadonlySet<string>
  onToggleChecked?: (id: string) => void
  /** Ids already rendered once; gates the one-shot entrance animation. */
  seenIds: Set<string>
  /** Currently previewed entry; its row gets the active highlight. */
  selectedId: string | null
  listRef?: React.RefObject<VirtuosoHandle | null>
  restoreStateFrom?: StateSnapshot | null
  isSearchActive: boolean
  submittedQuery: string
  searchLoading: boolean
  copySuccessId: string | null
  deletingIds: ReadonlySet<string>
  hasMore: boolean
  onLoadMore: () => void
  onCopy: (id: string) => void
  onFilePathsAction: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onCardClick: (id: string) => void
  onHoverChange: (id: string, hovered: boolean) => void
  onScrollStateRestored?: () => void
  /** Extra actions under the "no results" message of an active search. */
  emptyStateActions?: React.ReactNode
  /** Replaces the "no results" title and subtitle of an active search. */
  emptyStateText?: { title: string; subtitle: string }
}

/**
 * Scrollable list of history rows grouped under day headers, including its
 * loading and empty states. The list is virtualized, limiting mounted rows to
 * the visible window plus a small buffer.
 */
const HistoryGrid: React.FC<HistoryGridProps> = ({
  items,
  sourceDeviceNames,
  tagNames,
  checkedIds,
  onToggleChecked,
  seenIds,
  selectedId,
  listRef,
  restoreStateFrom,
  isSearchActive,
  submittedQuery,
  searchLoading,
  copySuccessId,
  deletingIds,
  hasMore,
  onLoadMore,
  onCopy,
  onFilePathsAction,
  onDelete,
  onToggleFavorite,
  onCardClick,
  onHoverChange,
  onScrollStateRestored,
  emptyStateActions,
  emptyStateText,
}) => {
  const { t } = useTranslation()
  // Loaded rows per calendar day, for the day headers.
  const dayCounts = React.useMemo(() => {
    const counts = new Map<number, number>()
    for (const item of items) {
      const key = dayKey(item.activeTime)
      counts.set(key, (counts.get(key) ?? 0) + 1)
    }
    return counts
  }, [items])

  return (
    <div className="@container flex-1 min-h-0 overflow-hidden">
      {searchLoading && items.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
          <Loader2 className="size-5 text-muted-foreground/40 animate-spin" />
          <p className="text-ui-body text-muted-foreground/50">{t('clipboard.search.searching')}</p>
        </div>
      ) : items.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
          <div className="size-12 rounded-2xl bg-muted/30 flex items-center justify-center">
            <Search className="size-5 text-muted-foreground/30" />
          </div>
          <div className="text-center space-y-1">
            {isSearchActive ? (
              <>
                <p className={cn('text-ui-section', emptyStateText && 'text-foreground')}>
                  {emptyStateText?.title ??
                    (submittedQuery.trim()
                      ? t('clipboard.search.noResults', {
                          query: submittedQuery,
                        })
                      : t('clipboard.search.noResultsFiltered'))}
                </p>
                <p className="text-ui-body text-muted-foreground/50">
                  {emptyStateText?.subtitle ?? t('clipboard.search.noResultsSub')}
                </p>
                {emptyStateActions && <div className="pt-3">{emptyStateActions}</div>}
              </>
            ) : (
              <>
                <p className="text-ui-section">{t('clipboard.content.noClipboardItems')}</p>
                <p className="text-ui-body text-muted-foreground/70">
                  {t('clipboard.content.emptyDescription')}
                </p>
              </>
            )}
          </div>
        </div>
      ) : (
        <Virtuoso
          ref={listRef}
          components={historyScrollComponents}
          data={items}
          style={{ height: '100%' }}
          className="flex-1 min-h-0"
          computeItemKey={(_index, item) => item.id}
          restoreStateFrom={restoreStateFrom ?? undefined}
          increaseViewportBy={{ top: 240, bottom: 480 }}
          itemsRendered={() => {
            if (restoreStateFrom) onScrollStateRestored?.()
          }}
          endReached={() => {
            if (hasMore && !searchLoading) onLoadMore()
          }}
          itemContent={(index, item) => {
            const opensDay =
              index === 0 || dayKey(items[index - 1].activeTime) !== dayKey(item.activeTime)
            const next = items[index + 1] as DisplayClipboardItem | undefined
            const isChecked = (id: string) => checkedIds?.has(id) ?? false
            const checked = isChecked(item.id)
            return (
              <HistoryGridRow
                item={item}
                dayStart={opensDay ? item.activeTime : undefined}
                // Consecutive checked rows of one day read as one block.
                joinsPrevious={checked && !opensDay && index > 0 && isChecked(items[index - 1].id)}
                joinsNext={
                  checked &&
                  next !== undefined &&
                  isChecked(next.id) &&
                  dayKey(next.activeTime) === dayKey(item.activeTime)
                }
                dayCount={dayCounts.get(dayKey(item.activeTime))}
                deviceName={
                  item.sourceDeviceId
                    ? deviceLabel(sourceDeviceNames?.[item.sourceDeviceId], item.sourceDeviceId)
                    : undefined
                }
                tagNames={tagNames}
                seenIds={seenIds}
                isActive={item.id === selectedId}
                copySuccess={copySuccessId === item.id}
                checked={checked}
                anyChecked={(checkedIds?.size ?? 0) > 0}
                onToggleChecked={onToggleChecked}
                isDeleting={deletingIds.has(item.id)}
                onCopy={onCopy}
                onFilePathsAction={onFilePathsAction}
                onDelete={onDelete}
                onToggleFavorite={onToggleFavorite}
                onClick={onCardClick}
                onHoverChange={onHoverChange}
              />
            )
          }}
        />
      )}
    </div>
  )
}

export default HistoryGrid
