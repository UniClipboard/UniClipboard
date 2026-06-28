import { m } from 'framer-motion'
import React, { useEffect } from 'react'
import HistoryCard from '@/components/history/HistoryCard'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'

interface HistoryGridRowProps {
  item: DisplayClipboardItem
  /** Ids already mounted once; gates the one-shot entrance animation. */
  seenIds: Set<string>
  isActive: boolean
  isHovered: boolean
  copySuccess: boolean
  isDeleting: boolean
  showDivider: boolean
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onClick: (id: string) => void
  onHoverChange: (id: string | null) => void
}

function rowHeightClass(item: DisplayClipboardItem): string {
  switch (item.type) {
    case 'text':
      return 'h-20'
    default:
      return 'h-24'
  }
}

const HistoryGridRow: React.FC<HistoryGridRowProps> = React.memo(
  ({
    item,
    seenIds,
    isActive,
    isHovered,
    copySuccess,
    isDeleting,
    showDivider,
    onCopy,
    onDelete,
    onToggleFavorite,
    onClick,
    onHoverChange,
  }) => {
    const isNew = !seenIds.has(item.id)

    useEffect(() => {
      seenIds.add(item.id)
    }, [item.id, seenIds])

    return (
      <m.div
        initial={isNew ? { opacity: 0, y: 16 } : false}
        animate={{ opacity: 1, y: 0 }}
        transition={{ type: 'spring', stiffness: 400, damping: 30 }}
        className={cn(
          rowHeightClass(item),
          'relative overflow-hidden transition-colors',
          showDivider && 'border-b border-border/40',
          isActive && 'bg-primary/[0.06]'
        )}
      >
        <HistoryCard
          item={item}
          isHovered={isHovered}
          copySuccess={copySuccess}
          isDeleting={isDeleting}
          onCopy={onCopy}
          onDelete={onDelete}
          onToggleFavorite={onToggleFavorite}
          onClick={onClick}
          onHoverChange={onHoverChange}
        />
      </m.div>
    )
  }
)

HistoryGridRow.displayName = 'HistoryGridRow'

export default HistoryGridRow
