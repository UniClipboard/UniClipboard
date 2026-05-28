import { AnimatePresence, m } from 'framer-motion'
import { Check, Copy, Download, Loader2, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Kbd } from '@/components/ui/kbd'
import { cn } from '@/lib/utils'

export interface ClipboardActionBarTransferStatus {
  isDownloaded?: boolean
  isTransferring?: boolean
  isCopyBlocked?: boolean
  copyBlockedReason?: string
}

interface ClipboardActionBarProps {
  hasActiveItem: boolean
  copySuccess: boolean
  activeItemType?: 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'
  transferStatus?: ClipboardActionBarTransferStatus
  onCopy: () => void
  onDelete: () => void
  onSyncToClipboard?: () => void
}

const ClipboardActionBar: React.FC<ClipboardActionBarProps> = ({
  hasActiveItem,
  copySuccess,
  activeItemType,
  transferStatus,
  onCopy,
  onDelete,
  onSyncToClipboard,
}) => {
  const {
    isDownloaded: isActiveItemDownloaded,
    isTransferring: isActiveItemTransferring,
    isCopyBlocked,
    copyBlockedReason,
  } = transferStatus ?? {}
  const { t } = useTranslation()

  // Show "Sync to Clipboard" instead of Copy for undownloaded file items
  const showSyncButton =
    activeItemType === 'file' && isActiveItemDownloaded === false && onSyncToClipboard

  return (
    <div
      className={cn(
        'flex items-center justify-end gap-1 w-full',
        !hasActiveItem && 'opacity-20 transition-opacity'
      )}
    >
      {showSyncButton ? (
        <m.button
          whileTap={{ scale: 0.97 }}
          className={cn(
            'flex items-center gap-2 px-2.5 py-1 rounded-md text-xs transition-colors duration-200',
            hasActiveItem && !isActiveItemTransferring
              ? 'text-primary hover:bg-primary/5'
              : 'text-muted-foreground/30 cursor-default'
          )}
          onClick={hasActiveItem && !isActiveItemTransferring ? onSyncToClipboard : undefined}
          disabled={!hasActiveItem || isActiveItemTransferring}
        >
          {isActiveItemTransferring ? (
            <Loader2 className="size-3 animate-spin" />
          ) : (
            <Download className="size-3" />
          )}
          <span className="font-medium whitespace-nowrap">
            {isActiveItemTransferring
              ? t('clipboard.actionBar.syncing')
              : t('clipboard.actionBar.syncToClipboard')}
          </span>
        </m.button>
      ) : (
        <m.button
          whileTap={{ scale: 0.97 }}
          className={cn(
            'flex items-center gap-2 px-2.5 py-1 rounded-md text-xs transition-all duration-200 relative group',
            hasActiveItem && !isCopyBlocked
              ? 'text-foreground hover:bg-muted'
              : 'text-muted-foreground/30 cursor-default'
          )}
          onClick={hasActiveItem && !isCopyBlocked ? onCopy : undefined}
          disabled={!hasActiveItem || isCopyBlocked}
          aria-disabled={isCopyBlocked}
          title={copyBlockedReason || undefined}
        >
          <AnimatePresence mode="wait" initial={false}>
            {copySuccess ? (
              <m.div
                key="check"
                initial={{ scale: 0.5, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.5, opacity: 0 }}
                transition={{ duration: 0.1 }}
              >
                <Check className="size-3 text-green-500" />
              </m.div>
            ) : (
              <m.div
                key="copy"
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.8, opacity: 0 }}
                transition={{ duration: 0.1 }}
              >
                <Copy className="size-3" />
              </m.div>
            )}
          </AnimatePresence>
          <span
            className={cn(
              'font-medium transition-colors whitespace-nowrap',
              copySuccess ? 'text-green-600 dark:text-green-400' : ''
            )}
          >
            {copyBlockedReason ||
              (copySuccess
                ? t('clipboard.actionBar.copied', '已复制')
                : t('clipboard.actionBar.copy'))}
          </span>
          {!isCopyBlocked && hasActiveItem && (
            <Kbd className="bg-transparent opacity-20 group-hover:opacity-100 transition-opacity border-none h-3 min-w-3 p-0 text-[9px]">
              C
            </Kbd>
          )}
        </m.button>
      )}

      <m.button
        whileTap={{ scale: 0.97 }}
        className={cn(
          'flex items-center gap-2 px-2.5 py-1 rounded-md text-xs transition-all duration-200 group',
          hasActiveItem
            ? 'text-muted-foreground hover:text-destructive hover:bg-destructive/5'
            : 'text-muted-foreground/30 cursor-default'
        )}
        onClick={hasActiveItem ? onDelete : undefined}
        disabled={!hasActiveItem}
      >
        <Trash2 className="size-3" />
        <span className="font-medium whitespace-nowrap">{t('clipboard.actionBar.delete')}</span>
        {hasActiveItem && (
          <Kbd className="bg-transparent opacity-20 group-hover:opacity-100 transition-opacity border-none h-3 min-w-3 p-0 text-[9px]">
            D
          </Kbd>
        )}
      </m.button>
    </div>
  )
}

export default ClipboardActionBar
