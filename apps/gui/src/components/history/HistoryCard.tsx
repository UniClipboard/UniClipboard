import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
import HistoryCardTags from './history-card/HistoryCardTags'
import HistoryCardTransferProgress from './history-card/HistoryCardTransferProgress'

interface HistoryCardProps {
  item: DisplayClipboardItem
  copySuccess: boolean
  isDeleting: boolean
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onClick: (id: string) => void
  onHoverChange: (id: string, hovered: boolean) => void
}

const HistoryCard: React.FC<HistoryCardProps> = ({
  item,
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
  // Directory sends reverse-report every member onto one transfer id, so the
  // byte percentage resets on each member switch — it is meaningless. Render
  // status only for those rows. Receiving directories keep their real
  // per-member aggregate percentage; single-file / flat sends are unaffected.
  const hideByteProgress = (item.isDirectory ?? false) && transfer?.direction === 'sending'

  // Keep hover and keyboard focus local so scrolling across rows does not
  // render the history page and its selected preview.
  const [isHovered, setIsHovered] = useState(false)
  const [focusWithin, setFocusWithin] = useState(false)
  useEffect(() => () => onHoverChange(item.id, false), [item.id, onHoverChange])
  const handleMouseEnter = useCallback(() => {
    setIsHovered(true)
    onHoverChange(item.id, true)
  }, [item.id, onHoverChange])
  const handleMouseLeave = useCallback(() => {
    setIsHovered(false)
    onHoverChange(item.id, false)
  }, [item.id, onHoverChange])
  const handleClick = useCallback(() => onClick(item.id), [item.id, onClick])
  const handleActionComplete = useCallback(() => {
    setFocusWithin(false)
    setIsHovered(false)
    onHoverChange(item.id, false)
  }, [item.id, onHoverChange])
  const handleFocus = useCallback(() => setFocusWithin(true), [])
  const handleBlur = useCallback((e: React.FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusWithin(false)
  }, [])
  const showActions = isHovered || focusWithin

  return (
    <div
      data-testid="history-card"
      data-entry-id={item.id}
      data-favorited={isFavorited}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onFocus={handleFocus}
      onBlur={handleBlur}
      className={cn(
        'group relative flex cursor-pointer flex-col overflow-hidden px-3.5 py-2.5 transition-all duration-200',
        item.type === 'text' ? 'min-h-24' : 'min-h-28',
        isTransferring && transfer && !hideByteProgress && 'pb-8',
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
        hideByteProgress={hideByteProgress}
      />
      <HistoryCardHeader
        item={item}
        relativeTime={relativeTime}
        transfer={transfer}
        state={cardState}
        percent={percent}
        hideByteProgress={hideByteProgress}
      />
      <div
        className={cn(
          'pointer-events-none relative z-10 shrink-0 flex-1 overflow-hidden',
          isPending && 'opacity-60'
        )}
      >
        <HistoryCardContent item={item} />
      </div>
      <HistoryCardTags tags={item.contentTags} />
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
