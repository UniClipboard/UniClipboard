import { Copy, FolderOpen, Loader2, Send, Star, Trash2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuPortal,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { toast } from '@/components/ui/toast'
import { useResendAction } from '@/hooks/useResendAction'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { firstRevealableFilePath } from '@/lib/clipboard-utils'
import { createLogger } from '@/lib/logger'
import { useAppSelector } from '@/store/hooks'

const log = createLogger('history-context-menu')

interface HistoryCardContextMenuProps {
  children: React.ReactNode
  item: DisplayClipboardItem
  /** Copy the entry to the local clipboard. */
  onCopy: (id: string) => void
  /** Toggle the entry's favorite flag; `current` is its present state. */
  onToggleFavorite: (id: string, current: boolean) => void
  /** Open the delete-confirmation flow for the entry. */
  onDelete: (id: string) => void
}

/**
 * Right-click context menu for a history grid row.
 *
 * Wraps a single card and exposes the per-entry actions the hover action bar
 * can't fit: copy, favorite, send-to-device (a submenu of paired peers plus an
 * "all" fan-out), reveal-in-folder for file entries, and delete.
 *
 * Copy / favorite / delete are delegated to the controller (it owns the
 * optimistic state, copy feedback, and the delete dialog). Send and reveal are
 * self-contained here — "send" reuses the resend pipeline (dispatch to peers
 * without writing the local clipboard) and reveal calls the native command —
 * so the grid doesn't have to thread two more handlers through every layer.
 */
const HistoryCardContextMenu: React.FC<HistoryCardContextMenuProps> = ({
  children,
  item,
  onCopy,
  onToggleFavorite,
  onDelete,
}) => {
  const { t } = useTranslation()
  const resendAction = useResendAction()
  // Paired space members back the "Send → device" submenu. Loaded into the
  // store by the history controller on mount, so this just reads the snapshot.
  const members = useAppSelector(state => state.devices.spaceMembers)

  const isFavorited = item.isFavorited ?? false
  const isUnavailable = item.isUnavailable ?? false
  const revealPath = item.type === 'file' ? firstRevealableFilePath(item.content) : null
  const sendInFlight = resendAction.isEntryInFlight(item.id)

  const handleOpenFileLocation = async () => {
    if (!revealPath) return
    try {
      const { revealPath: reveal } = await import('@/api/storage')
      await reveal(revealPath)
    } catch (err) {
      log.warn({ err, entryId: item.id }, 'failed to reveal file location')
      toast.error(t('clipboard.errors.openLocationFailed'))
    }
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger className="block h-full w-full">{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-48">
        {/* Copy — unavailable entries (payload lost) can't be restored. */}
        <ContextMenuItem disabled={isUnavailable} onClick={() => !isUnavailable && onCopy(item.id)}>
          <Copy className="mr-2 size-4" />
          {t('clipboard.contextMenu.copy')}
          <ContextMenuShortcut>C</ContextMenuShortcut>
        </ContextMenuItem>

        {/* Favorite toggle */}
        <ContextMenuItem onClick={() => onToggleFavorite(item.id, isFavorited)}>
          <Star className={`mr-2 size-4 ${isFavorited ? 'fill-current' : ''}`} />
          {isFavorited
            ? t('clipboard.contextMenu.unfavorite')
            : t('clipboard.contextMenu.favorite')}
          <ContextMenuShortcut>F</ContextMenuShortcut>
        </ContextMenuItem>

        {/* Send — dispatch to a peer (or all) without touching the local clipboard.
            Unavailable entries (payload lost) can't be resent, so gate it like copy. */}
        <ContextMenuSub>
          <ContextMenuSubTrigger disabled={sendInFlight || isUnavailable}>
            {sendInFlight ? (
              <Loader2 className="mr-2 size-4 animate-spin" />
            ) : (
              <Send className="mr-2 size-4" />
            )}
            {t('clipboard.contextMenu.send')}
          </ContextMenuSubTrigger>
          <ContextMenuPortal>
            <ContextMenuSubContent className="w-44">
              {members.length === 0 ? (
                <ContextMenuItem disabled>
                  {t('clipboard.contextMenu.sendNoDevices')}
                </ContextMenuItem>
              ) : (
                <>
                  <ContextMenuItem onClick={() => void resendAction.resendAll(item.id)}>
                    {t('clipboard.contextMenu.sendAll')}
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                  {members.map(member => (
                    <ContextMenuItem
                      key={member.peerId}
                      disabled={!member.connected}
                      onClick={() => void resendAction.resendToPeer(item.id, member.peerId)}
                    >
                      <span className="truncate">{member.deviceName}</span>
                    </ContextMenuItem>
                  ))}
                </>
              )}
            </ContextMenuSubContent>
          </ContextMenuPortal>
        </ContextMenuSub>

        {/* Open file location — file entries with at least one revealable path. */}
        {revealPath && (
          <ContextMenuItem onClick={() => void handleOpenFileLocation()}>
            <FolderOpen className="mr-2 size-4" />
            {t('clipboard.contextMenu.openFileLocation')}
          </ContextMenuItem>
        )}

        <ContextMenuSeparator />

        {/* Delete */}
        <ContextMenuItem variant="destructive" onClick={() => onDelete(item.id)}>
          <Trash2 className="mr-2 size-4" />
          {t('clipboard.contextMenu.delete')}
          <ContextMenuShortcut>D</ContextMenuShortcut>
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

export default HistoryCardContextMenu
