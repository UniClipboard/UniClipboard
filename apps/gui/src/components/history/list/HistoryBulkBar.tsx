import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/motion/context-menu'
import { useResendAction } from '@/hooks/useResendAction'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'

interface HistoryBulkBarProps {
  items: DisplayClipboardItem[]
  onPin: () => void
  onDelete: () => void
}

const ACTION_CLASS =
  'inline-flex h-9 shrink-0 items-center rounded-lg px-3 text-ui-body font-medium transition-colors hover:bg-background/12 disabled:opacity-50'

/**
 * HList.dc.html bulk bar: floats over the list bottom while rows are checked
 * and acts on all of them. Tagging is left out: there is no tag assignment
 * API yet.
 */
function HistoryBulkBar({ items, onPin, onDelete }: HistoryBulkBarProps) {
  const { t } = useTranslation()
  const members = useAppSelector(state => state.devices.spaceMembers)
  const action = useResendAction()
  const ids = items.map(item => item.id)
  const unpin = items.every(item => item.isFavorited === true)

  return (
    <div
      role="toolbar"
      aria-label={t('history.list.bulkActions')}
      className="absolute inset-x-3 bottom-3 z-20 flex h-13 items-center gap-1 rounded-2xl bg-foreground pl-4 pr-2 text-background shadow-xl animate-in fade-in-0 slide-in-from-bottom-2 duration-150"
    >
      <span className="min-w-0 flex-1 truncate text-ui-body font-semibold">
        {t('history.list.selected', { count: items.length })}
      </span>
      <button type="button" className={ACTION_CLASS} onClick={onPin}>
        {t(unpin ? 'clipboard.contextMenu.unfavorite' : 'clipboard.contextMenu.favorite')}
      </button>
      <ContextMenu>
        <ContextMenuTrigger activation="click">
          <button type="button" className={ACTION_CLASS}>
            {t('history.list.sendTo')}
          </button>
        </ContextMenuTrigger>
        <ContextMenuContent
          side="top"
          ariaLabel={t('clipboard.contextMenu.send')}
          className="w-56 max-w-[calc(100vw-2rem)]"
        >
          <ContextMenuLabel>{t('clipboard.contextMenu.send')}</ContextMenuLabel>
          {members.length === 0 ? (
            <ContextMenuItem disabled>{t('clipboard.contextMenu.sendNoDevices')}</ContextMenuItem>
          ) : (
            <>
              <ContextMenuItem
                disabled={!members.some(member => member.connected)}
                onSelect={() => void action.resendMany(ids)}
              >
                {t('clipboard.contextMenu.sendAll')}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <div className="flex max-h-60 flex-col overflow-y-auto">
                {members.map(member => (
                  <ContextMenuItem
                    key={member.peerId}
                    disabled={!member.connected}
                    onSelect={() => void action.resendMany(ids, member.peerId)}
                    textValue={member.deviceName}
                  >
                    <span className="truncate">{member.deviceName}</span>
                  </ContextMenuItem>
                ))}
              </div>
            </>
          )}
        </ContextMenuContent>
      </ContextMenu>
      <button
        type="button"
        className={cn(ACTION_CLASS, 'font-semibold text-orange-300 dark:text-orange-700')}
        onClick={onDelete}
      >
        {t('clipboard.contextMenu.delete')}
      </button>
    </div>
  )
}

export default HistoryBulkBar
