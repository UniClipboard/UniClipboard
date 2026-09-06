import { Check, Copy, Star, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { ExpandableActionBar } from '@/components/motion/expandable-action-bar'
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
    <ExpandableActionBar
      size="sm"
      items={[
        {
          id: 'copy',
          label:
            copyBlockedReason ||
            (copySuccess
              ? t('clipboard.actionBar.copied', '已复制')
              : t('clipboard.actionBar.copy')),
          icon: copySuccess ? (
            <Check className="size-3.5 text-green-600 dark:text-green-400" />
          ) : (
            <Copy className="size-3.5" />
          ),
          shortcut: !isCopyBlocked && hasActiveItem ? 'C' : undefined,
          disabled: !hasActiveItem || isCopyBlocked,
          onClick: onCopy,
        },
        {
          id: 'favorite',
          label: favoriteLabel,
          icon: (
            <Star
              className={cn(
                'size-3.5',
                isFavorited && 'fill-current text-amber-600 dark:text-amber-400'
              )}
            />
          ),
          shortcut: hasActiveItem ? 'F' : undefined,
          active: isFavorited,
          disabled: !hasActiveItem,
          onClick: onToggleFavorite,
        },
        {
          id: 'delete',
          label: t('clipboard.actionBar.delete'),
          icon: <Trash2 className="size-3.5" />,
          shortcut: hasActiveItem ? 'D' : undefined,
          disabled: !hasActiveItem,
          onClick: onDelete,
        },
      ]}
      classNames={{
        track: 'min-h-7 border-0 bg-transparent p-0 shadow-none backdrop-blur-none',
        item: 'hover:text-foreground',
      }}
    />
  )
}

export default ClipboardActionBar
