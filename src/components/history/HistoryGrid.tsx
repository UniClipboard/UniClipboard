import { m } from 'framer-motion'
import { Loader2, Search } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import HistoryCard from '@/components/history/HistoryCard'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

interface HistoryGridProps {
  scrollRef: React.RefObject<HTMLDivElement | null>
  items: DisplayClipboardItem[]
  /** Ids already rendered once; gates the one-shot entrance animation. */
  seenIds: Set<string>
  isSearchActive: boolean
  submittedQuery: string
  searchLoading: boolean
  hoveredId: string | null
  copySuccessId: string | null
  deletingId: string | null
  onCopy: (id: string) => void
  onCardClick: (id: string) => void
  onHoverChange: (id: string | null) => void
}

/**
 * Scrollable card grid for the history view, including its loading and empty
 * states. The owning page supplies the scroll ref (so it can drive infinite
 * scroll) and the per-card interaction handlers.
 */
const HistoryGrid: React.FC<HistoryGridProps> = ({
  scrollRef,
  items,
  seenIds,
  isSearchActive,
  submittedQuery,
  searchLoading,
  hoveredId,
  copySuccessId,
  deletingId,
  onCopy,
  onCardClick,
  onHoverChange,
}) => {
  const { t } = useTranslation()

  return (
    <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
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
        <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] items-start gap-x-3 gap-y-2 px-3 pt-2 pb-4">
          {items.map(item => {
            const isNew = !seenIds.has(item.id)
            return (
              <m.div
                key={item.id}
                initial={isNew ? { opacity: 0, y: 16 } : false}
                animate={{ opacity: 1, y: 0 }}
                transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                className="h-44 rounded-xl border border-border/40 bg-card/40 overflow-hidden"
              >
                <HistoryCard
                  item={item}
                  isHovered={hoveredId === item.id}
                  copySuccess={copySuccessId === item.id}
                  isDeleting={deletingId === item.id}
                  onCopy={onCopy}
                  onClick={onCardClick}
                  onHoverChange={onHoverChange}
                />
              </m.div>
            )
          })}
        </div>
      )}
    </div>
  )
}

export default HistoryGrid
