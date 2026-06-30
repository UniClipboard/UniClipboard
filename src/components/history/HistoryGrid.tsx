import { Loader2, Search } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Virtuoso, type StateSnapshot, type VirtuosoHandle } from 'react-virtuoso'
import HistoryGridRow from '@/components/history/HistoryGridRow'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

interface HistoryGridProps {
  items: DisplayClipboardItem[]
  /** Ids already rendered once; gates the one-shot entrance animation. */
  seenIds: Set<string>
  /** Currently previewed entry; its row gets the active highlight. */
  selectedId: string | null
  listRef?: React.RefObject<VirtuosoHandle | null>
  restoreStateFrom?: StateSnapshot | null
  isSearchActive: boolean
  submittedQuery: string
  searchLoading: boolean
  hoveredId: string | null
  copySuccessId: string | null
  deletingId: string | null
  hasMore: boolean
  onLoadMore: () => void
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onCardClick: (id: string) => void
  onHoverChange: (id: string | null) => void
  onScrollStateRestored?: () => void
}

/**
 * Scrollable card grid for the history view, including its loading and empty
 * states. The virtualized list keeps the row card behavior unchanged while
 * limiting mounted cards to the visible window plus a small buffer.
 */
const HistoryGrid: React.FC<HistoryGridProps> = ({
  items,
  seenIds,
  selectedId,
  listRef,
  restoreStateFrom,
  isSearchActive,
  submittedQuery,
  searchLoading,
  hoveredId,
  copySuccessId,
  deletingId,
  hasMore,
  onLoadMore,
  onCopy,
  onDelete,
  onToggleFavorite,
  onCardClick,
  onHoverChange,
  onScrollStateRestored,
}) => {
  const { t } = useTranslation()

  return (
    <div className="no-scrollbar flex-1 min-h-0 overflow-y-auto">
      {searchLoading && items.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
          <Loader2 className="size-5 text-muted-foreground/40 animate-spin" />
          <p className="text-[12px] text-muted-foreground/50">{t('clipboard.search.searching')}</p>
        </div>
      ) : items.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
          <div className="size-12 rounded-2xl bg-muted/30 flex items-center justify-center">
            <Search className="size-5 text-muted-foreground/30" />
          </div>
          <div className="text-center space-y-1">
            {isSearchActive ? (
              <>
                <p className="text-[13px] font-medium">
                  {submittedQuery.trim()
                    ? t('clipboard.search.noResults', { query: submittedQuery })
                    : t('clipboard.search.noResultsFiltered')}
                </p>
                <p className="text-[12px] text-muted-foreground/50">
                  {t('clipboard.search.noResultsSub')}
                </p>
              </>
            ) : (
              <>
                <p className="text-[13px] font-medium">{t('clipboard.content.noClipboardItems')}</p>
                <p className="text-[12px] text-muted-foreground/50">
                  {t('clipboard.content.emptyDescription')}
                </p>
              </>
            )}
          </div>
        </div>
      ) : (
        <Virtuoso
          ref={listRef}
          data={items}
          style={{ height: '100%' }}
          className="no-scrollbar flex-1 min-h-0"
          computeItemKey={(_index, item) => item.id}
          restoreStateFrom={restoreStateFrom ?? undefined}
          increaseViewportBy={{ top: 240, bottom: 480 }}
          itemsRendered={() => {
            if (restoreStateFrom) onScrollStateRestored?.()
          }}
          endReached={() => {
            if (hasMore && !searchLoading) onLoadMore()
          }}
          itemContent={(index, item) => (
            <HistoryGridRow
              item={item}
              seenIds={seenIds}
              isActive={item.id === selectedId}
              isHovered={hoveredId === item.id}
              copySuccess={copySuccessId === item.id}
              isDeleting={deletingId === item.id}
              showDivider={index < items.length - 1}
              onCopy={onCopy}
              onDelete={onDelete}
              onToggleFavorite={onToggleFavorite}
              onClick={onCardClick}
              onHoverChange={onHoverChange}
            />
          )}
        />
      )}
    </div>
  )
}

export default HistoryGrid
