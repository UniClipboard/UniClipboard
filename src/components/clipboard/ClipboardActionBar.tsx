import { Check, Copy, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Kbd } from '@/components/ui/kbd'
import { cn } from '@/lib/utils'

interface ClipboardActionBarProps {
  hasActiveItem: boolean
  copySuccess: boolean
  onCopy: () => void
  onDelete: () => void
}

const ClipboardActionBar: React.FC<ClipboardActionBarProps> = ({
  hasActiveItem,
  copySuccess,
  onCopy,
  onDelete,
}) => {
  const { t } = useTranslation()

  return (
    <div className="flex items-center justify-end border-t border-border/40 bg-card px-4 py-2 shrink-0">
      <div className="flex items-center gap-3">
        <button
          className={cn(
            'flex items-center gap-1.5 text-sm transition-colors',
            hasActiveItem
              ? 'text-foreground hover:text-primary cursor-pointer'
              : 'text-muted-foreground/50 cursor-default'
          )}
          onClick={hasActiveItem ? onCopy : undefined}
          disabled={!hasActiveItem}
        >
          {copySuccess ? (
            <Check className="h-4 w-4 text-green-500" />
          ) : (
            <Copy className="h-4 w-4" />
          )}
          <span>{t('clipboard.actionBar.copy')}</span>
          <Kbd>C</Kbd>
        </button>

        <div className="w-px h-4 bg-border" />

        <button
          className={cn(
            'flex items-center gap-1.5 text-sm transition-colors',
            hasActiveItem
              ? 'text-foreground hover:text-destructive cursor-pointer'
              : 'text-muted-foreground/50 cursor-default'
          )}
          onClick={hasActiveItem ? onDelete : undefined}
          disabled={!hasActiveItem}
        >
          <Trash2 className="h-4 w-4" />
          <span>{t('clipboard.actionBar.delete')}</span>
          <Kbd>D</Kbd>
        </button>
      </div>
    </div>
  )
}

export default ClipboardActionBar
