import { Copy, Star, Trash2 } from 'lucide-react'
import type { MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

interface HistoryCardActionsProps {
  itemId: string
  state: {
    isHovered: boolean
    isTransferring: boolean
    isPending: boolean
    isFavorited: boolean
  }
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onActionComplete: () => void
}

function HistoryCardActions({
  itemId,
  state,
  onCopy,
  onDelete,
  onToggleFavorite,
  onActionComplete,
}: HistoryCardActionsProps) {
  const { t } = useTranslation()
  const { isHovered, isTransferring, isPending, isFavorited } = state
  const actionBtnClass =
    'flex size-6 items-center justify-center rounded-lg text-muted-foreground/70 transition-colors hover:bg-foreground/10 hover:text-foreground'
  const runAction = (e: MouseEvent<HTMLButtonElement>, action: () => void) => {
    e.stopPropagation()
    action()
    e.currentTarget.blur()
    onActionComplete()
  }

  return (
    <div
      className={cn(
        'absolute bottom-1.5 right-2 z-20 flex items-center gap-0.5 rounded-xl border border-border/40 bg-card/95 p-0.5 shadow-sm backdrop-blur transition-opacity duration-150',
        isHovered && !isTransferring && !isPending ? 'opacity-100' : 'pointer-events-none opacity-0'
      )}
    >
      <button
        type="button"
        aria-label={t('clipboard.item.actions.copy')}
        tabIndex={isHovered ? 0 : -1}
        onClick={e => runAction(e, () => onCopy(itemId))}
        className={actionBtnClass}
      >
        <Copy className="size-3" />
      </button>
      <button
        type="button"
        aria-label={t(
          isFavorited ? 'clipboard.item.actions.unfavorite' : 'clipboard.item.actions.favorite'
        )}
        tabIndex={isHovered ? 0 : -1}
        onClick={e => runAction(e, () => onToggleFavorite(itemId, isFavorited))}
        className={actionBtnClass}
      >
        <Star className={cn('size-3', isFavorited && 'fill-amber-400 text-amber-400')} />
      </button>
      <button
        type="button"
        aria-label={t('clipboard.item.actions.delete')}
        tabIndex={isHovered ? 0 : -1}
        onClick={e => runAction(e, () => onDelete(itemId))}
        className={cn(actionBtnClass, 'hover:bg-destructive/10 hover:text-destructive')}
      >
        <Trash2 className="size-3" />
      </button>
    </div>
  )
}

export default HistoryCardActions
