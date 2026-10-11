import { useTranslation } from 'react-i18next'
import { useResendAction } from '@/hooks/useResendAction'
import { useAppSelector } from '@/store/hooks'
import { Menu, type MenuEntry } from '@/ui-kit'

const ALL_DEVICES = 'all'

/** "Send to…": resend the entries to every paired device or to one of them. */
export function HistoryKitSendMenu({
  entryIds,
  disabled,
  iconOnly,
}: {
  entryIds: string[]
  disabled?: boolean
  iconOnly?: boolean
}) {
  const { t } = useTranslation()
  const members = useAppSelector(state => state.devices.spaceMembers)
  const action = useResendAction()
  const busy = entryIds.some(id => action.isEntryInFlight(id))
  const items: MenuEntry[] =
    members.length === 0
      ? [{ id: 'none', label: t('clipboard.contextMenu.sendNoDevices'), disabled: true }]
      : [
          { id: ALL_DEVICES, label: t('clipboard.contextMenu.sendAll'), icon: 'devices' },
          { id: 'separator', kind: 'separator' },
          ...members.map(member => ({ id: member.peerId, label: member.deviceName })),
        ]
  const send = (id: string) => {
    const peer = id === ALL_DEVICES ? undefined : id
    if (entryIds.length > 1) void action.resendMany(entryIds, peer)
    else if (peer) void action.resendToPeer(entryIds[0], peer)
    else void action.resendAll(entryIds[0])
  }
  return (
    <Menu
      label={t('history.list.sendTo')}
      icon="send"
      iconOnly={iconOnly}
      variant={iconOnly ? 'subtle' : 'standard'}
      items={disabled || busy ? [] : items}
      onAction={send}
    />
  )
}
