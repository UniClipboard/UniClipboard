import { Loader2, Send } from 'lucide-react'
import { useState, type ReactElement } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/motion/context-menu'
import { ExpandableActionBar } from '@/components/motion/expandable-action-bar'
import { useResendAction } from '@/hooks/useResendAction'
import { useAppSelector } from '@/store/hooks'

interface ClipboardSendMenuProps {
  entryId: string
  disabled?: boolean
}

export default function ClipboardSendMenu({ entryId, disabled }: ClipboardSendMenuProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const members = useAppSelector(state => state.devices.spaceMembers)
  const action = useResendAction()
  const busy = action.isEntryInFlight(entryId)
  const send = (peerId?: string) => {
    setOpen(false)
    if (peerId) void action.resendToPeer(entryId, peerId)
    else void action.resendAll(entryId)
  }

  return (
    <ContextMenu open={open} onOpenChange={setOpen}>
      <ExpandableActionBar
        size="sm"
        expanded={open || expanded}
        onExpandedChange={setExpanded}
        items={[
          {
            id: 'send',
            label: t('clipboard.contextMenu.send'),
            icon: busy ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Send className="size-3.5" />
            ),
            disabled: disabled || busy,
            renderButton: (button: ReactElement<Record<string, unknown>>) => (
              <ContextMenuTrigger activation="click" disabled={disabled || busy}>
                {button}
              </ContextMenuTrigger>
            ),
          },
        ]}
        classNames={{
          track: 'min-h-7 border-0 bg-transparent p-0 shadow-none backdrop-blur-none',
          item: 'hover:text-foreground',
        }}
      />
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
              disabled={
                busy ||
                !members.some(member => member.connected) ||
                members.some(member => action.isPeerInFlight(entryId, member.peerId))
              }
              onSelect={() => send()}
            >
              {t('clipboard.contextMenu.sendAll')}
            </ContextMenuItem>
            <ContextMenuSeparator />
            <div className="flex max-h-60 flex-col overflow-y-auto">
              {members.map(member => (
                <ContextMenuItem
                  key={member.peerId}
                  disabled={
                    busy || !member.connected || action.isPeerInFlight(entryId, member.peerId)
                  }
                  onSelect={() => send(member.peerId)}
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
  )
}
