import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import { useRelativeTime } from '@/hooks/useRelativeTime'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import HistoryCardActions from './history-card/HistoryCardActions'
import HistoryCardContent from './history-card/HistoryCardContent'
import HistoryCardHeader from './history-card/HistoryCardHeader'
import HistoryCardTransferProgress from './history-card/HistoryCardTransferProgress'

interface HistoryCardProps {
  item: DisplayClipboardItem
  isHovered: boolean
  copySuccess: boolean
  isDeleting: boolean
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onClick: (id: string) => void
  onHoverChange: (id: string | null) => void
}

const HistoryCard: React.FC<HistoryCardProps> = ({
  item,
  isHovered,
  copySuccess,
  isDeleting,
  onCopy,
  onDelete,
  onToggleFavorite,
  onClick,
  onHoverChange,
}) => {
  const { t } = useTranslation()
  const relativeTime = useRelativeTime(item.activeTime)
  const { delivery } = useEntryDelivery(item.id)
  const isFileType = item.type === 'file'
  const isFavorited = item.isFavorited ?? false
  const isUnavailable = item.isUnavailable ?? false
  const transfer = useAppSelector(state =>
    isFileType ? selectTransferByEntryId(state, item.id) : undefined
  )
  const entryStatus = useAppSelector(state =>
    isFileType ? selectEntryTransferStatus(state, item.id) : undefined
  )
  const effectiveStatus = isFileType ? resolveEntryTransferStatus(entryStatus, transfer) : undefined
  const isTransferring = effectiveStatus === 'transferring'
  const isPending = effectiveStatus === 'pending'
  const cardState = { isFileType, isFavorited, isUnavailable, isTransferring, isPending }
  const percent =
    transfer && transfer.totalBytes && transfer.totalBytes > 0
      ? Math.round((transfer.bytesTransferred / transfer.totalBytes) * 100)
      : 0

  // Reveal the action bar on keyboard focus too, not just mouse hover — its
  // buttons are otherwise untabbable, so keyboard users could never reach
  // copy/favorite/delete. Tracked locally (separate from the parent's hover
  // selection) so it doesn't disturb the hover-driven keyboard shortcuts.
  const [focusWithin, setFocusWithin] = useState(false)
  const handleMouseEnter = useCallback(() => onHoverChange(item.id), [item.id, onHoverChange])
  const handleMouseLeave = useCallback(() => onHoverChange(null), [onHoverChange])
  const handleClick = useCallback(() => onClick(item.id), [item.id, onClick])
  const handleActionComplete = useCallback(() => {
    setFocusWithin(false)
    onHoverChange(null)
  }, [onHoverChange])
  const handleFocus = useCallback(() => setFocusWithin(true), [])
  const handleBlur = useCallback((e: React.FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusWithin(false)
  }, [])
  const showActions = isHovered || focusWithin

  return (
    <div
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onFocus={handleFocus}
      onBlur={handleBlur}
      className={cn(
        'group relative flex h-full cursor-pointer flex-col overflow-hidden px-3.5 py-2.5 transition-all duration-200',
        isDeleting
          ? 'bg-destructive/10 opacity-60 scale-[0.97]'
          : copySuccess
            ? 'bg-emerald-500/5'
            : isPending
              ? 'bg-muted/10'
              : 'hover:bg-muted/40',
        isUnavailable && 'opacity-55'
      )}
    >
      <button
        type="button"
        aria-label={t('clipboard.item.actions.open', 'Open clipboard item')}
        onClick={handleClick}
        className="absolute inset-0 z-[1] cursor-pointer appearance-none border-0 bg-transparent p-0 text-left outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
      />
      <HistoryCardTransferProgress
        isFileType={isFileType}
        isTransferring={isTransferring}
        transfer={transfer}
        percent={percent}
      />
      <HistoryCardHeader
        item={item}
        relativeTime={relativeTime}
        deliverySource={delivery?.source}
        transfer={transfer}
        state={cardState}
        percent={percent}
      />
      <div
        className={cn(
          'pointer-events-none relative z-10 min-h-0 flex-1 overflow-hidden',
          isPending && 'opacity-60'
        )}
      >
        <HistoryCardContent item={item} />
      </div>
      <HistoryCardActions
        itemId={item.id}
        state={{ isHovered: showActions, isTransferring, isPending, isFavorited }}
        onCopy={onCopy}
        onDelete={onDelete}
        onToggleFavorite={onToggleFavorite}
        onActionComplete={handleActionComplete}
      />
    </div>
  )
}

export default HistoryCard
