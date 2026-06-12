import { AnimatePresence, m } from 'framer-motion'
import { Check, Copy, Trash2 } from 'lucide-react'
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
  transferStatus?: ClipboardActionBarTransferStatus
  onCopy: () => void
  onDelete: () => void
}

const ClipboardActionBar: React.FC<ClipboardActionBarProps> = ({
  hasActiveItem,
  copySuccess,
  transferStatus,
  onCopy,
  onDelete,
}) => {
  const { isCopyBlocked, copyBlockedReason } = transferStatus ?? {}
  const { t } = useTranslation()

  return (
    <div
      className={cn(
        'flex items-center justify-end gap-1 w-full',
        !hasActiveItem && 'opacity-20 transition-opacity'
      )}
    >
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
