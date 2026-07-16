import { AnimatePresence, m } from 'framer-motion'
import { Check, Copy, Star, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Kbd } from '@/components/ui/kbd'
import { cn } from '@/lib/utils'

export interface ClipboardActionBarTransferStatus {
  isCopyBlocked?: boolean
  copyBlockedReason?: string
}

interface ClipboardActionBarProps {
  hasActiveItem: boolean
  copySuccess: boolean
  isFavorited: boolean
  transferStatus?: ClipboardActionBarTransferStatus
  onCopy: () => void
  onDelete: () => void
  onToggleFavorite: () => void
}

const ClipboardActionBar: React.FC<ClipboardActionBarProps> = ({
  hasActiveItem,
  copySuccess,
  isFavorited,
  transferStatus,
  onCopy,
  onDelete,
  onToggleFavorite,
}) => {
  const { isCopyBlocked, copyBlockedReason } = transferStatus ?? {}
  const { t } = useTranslation()
  const favoriteLabel = isFavorited
    ? t('clipboard.actionBar.unfavorite')
    : t('clipboard.actionBar.favorite')

  return (
    <div
      className={cn(
        'flex items-center justify-end gap-1 w-full',
        !hasActiveItem && 'opacity-20 transition-opacity'
      )}
    >
      <m.button
        type="button"
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
        aria-label={copyBlockedReason || t('clipboard.actionBar.copy')}
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

      <m.button
        type="button"
        whileTap={{ scale: 0.97 }}
        className={cn(
          'flex items-center gap-2 px-2.5 py-1 rounded-md text-xs transition-all duration-200 group',
          hasActiveItem
            ? isFavorited
              ? 'text-amber-600 hover:bg-amber-500/10 dark:text-amber-400'
              : 'text-muted-foreground hover:bg-muted hover:text-foreground'
            : 'text-muted-foreground/30 cursor-default'
        )}
        onClick={hasActiveItem ? onToggleFavorite : undefined}
        disabled={!hasActiveItem}
        aria-pressed={isFavorited}
        aria-label={favoriteLabel}
      >
        <Star className={cn('size-3', isFavorited && 'fill-current')} />
        <span className="font-medium whitespace-nowrap">{favoriteLabel}</span>
        {hasActiveItem && (
          <Kbd className="bg-transparent opacity-20 group-hover:opacity-100 transition-opacity border-none h-3 min-w-3 p-0 text-[9px]">
            F
          </Kbd>
        )}
      </m.button>

      <m.button
        type="button"
        whileTap={{ scale: 0.97 }}
        className={cn(
          'flex items-center gap-2 px-2.5 py-1 rounded-md text-xs transition-all duration-200 group',
          hasActiveItem
            ? 'text-muted-foreground hover:text-destructive hover:bg-destructive/5'
            : 'text-muted-foreground/30 cursor-default'
        )}
        onClick={hasActiveItem ? onDelete : undefined}
        disabled={!hasActiveItem}
        aria-label={t('clipboard.actionBar.delete')}
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
