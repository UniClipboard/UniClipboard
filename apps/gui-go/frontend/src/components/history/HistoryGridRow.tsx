import { m } from 'framer-motion'
import React, { useEffect } from 'react'
import { HISTORY_ENTRY_ANIMATION } from '@/components/history/history-entry-animation'
import HistoryCardContextMenu from '@/components/history/HistoryCardContextMenu'
import HistoryDayHeader from '@/components/history/list/HistoryDayHeader'
import HistoryListRow from '@/components/history/list/HistoryListRow'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'

const noop = () => {}

interface HistoryGridRowProps {
  item: DisplayClipboardItem
  /** The row's timestamp when it opens a new calendar day. */
  dayStart?: number
  /** How many loaded rows share `dayStart`'s day. */
  dayCount?: number
  /** The origin device's name, when known. */
  deviceName?: string
  /** Local tag id -> name, for the row's tag chips. */
  tagNames?: ReadonlyMap<string, string | null>
  /** Bulk selection state and toggle. */
  checked?: boolean
  anyChecked?: boolean
  onToggleChecked?: (id: string) => void
  /** The row above / below is also checked and in the same day,
   * so the shared edge drops its rounding and the run reads as one block. */
  joinsPrevious?: boolean
  joinsNext?: boolean
  /** Ids already mounted once; gates the one-shot entrance animation. */
  seenIds: Set<string>
  isActive: boolean
  copySuccess: boolean
  isDeleting: boolean
  onCopy: (id: string) => void
  onFilePathsAction: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onClick: (id: string) => void
  onHoverChange: (id: string, hovered: boolean) => void
}

const HistoryGridRow: React.FC<HistoryGridRowProps> = React.memo(
  ({
    item,
    dayStart,
    dayCount = 0,
    deviceName,
    tagNames,
    checked = false,
    anyChecked = false,
    onToggleChecked,
    joinsPrevious = false,
    joinsNext = false,
    seenIds,
    isActive,
    copySuccess,
    isDeleting,
    onCopy,
    onFilePathsAction,
    onDelete,
    onToggleFavorite,
    onClick,
    onHoverChange,
  }) => {
    const isNew = !seenIds.has(item.id)

    useEffect(() => {
      seenIds.add(item.id)
    }, [item.id, seenIds])

    const row = (
      <m.div
        data-testid="history-row"
        initial={isNew ? HISTORY_ENTRY_ANIMATION.initial : false}
        animate={HISTORY_ENTRY_ANIMATION.animate}
        transition={HISTORY_ENTRY_ANIMATION.transition}
        className={cn(
          'relative overflow-hidden transition-colors',
          // Rows are inset rounded blocks; selection is their fill.
          'mx-2 rounded-[0.625rem]',
          joinsPrevious && 'rounded-t-none',
          joinsNext && 'rounded-b-none',
          isActive && 'bg-(--history-selection-background)'
        )}
      >
        <HistoryCardContextMenu
          item={item}
          onCopy={onCopy}
          onFilePathsAction={onFilePathsAction}
          onToggleFavorite={onToggleFavorite}
          onDelete={onDelete}
        >
          <HistoryListRow
            item={item}
            deviceName={deviceName}
            tagNames={tagNames}
            checked={checked}
            anyChecked={anyChecked}
            onToggleChecked={onToggleChecked ?? noop}
            copySuccess={copySuccess}
            isDeleting={isDeleting}
            onClick={onClick}
            onHoverChange={onHoverChange}
          />
        </HistoryCardContextMenu>
      </m.div>
    )
    if (dayStart === undefined) return row
    return (
      <>
        <HistoryDayHeader at={dayStart} count={dayCount} />
        {row}
      </>
    )
  }
)

HistoryGridRow.displayName = 'HistoryGridRow'

export default HistoryGridRow
