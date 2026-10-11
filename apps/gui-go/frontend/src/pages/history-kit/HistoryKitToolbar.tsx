import { useTranslation } from 'react-i18next'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { Menu, Toolbar, ToolbarButton } from '@/ui-kit'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

const MANAGE_TAGS = 'manage-tags'
const DELETE = 'delete'

interface HistoryKitToolbarProps {
  /** The entries the item commands act on: the selection. */
  targetIds: string[]
  pinned: boolean
  onTogglePin: () => void
  onDelete: () => void
  onManageTags?: () => void
}

/**
 * The item commands of the window toolbar (design MA2): Send, Pin and More for the selection. The design's list / grid switch is not shown: the grid layout (ME2) is not
 * built yet.
 */
export function HistoryKitToolbar({
  targetIds,
  pinned,
  onTogglePin,
  onDelete,
  onManageTags,
}: HistoryKitToolbarProps) {
  const { t } = useTranslation()
  const none = targetIds.length === 0
  return (
    <Toolbar label={t('history.kit.toolbar')}>
      <HistoryKitSendMenu entryIds={targetIds} disabled={none} iconOnly />
      <ToolbarButton
        icon="pin"
        label={t(pinned ? 'history.detail.unpin' : 'history.detail.pin')}
        tooltip={`${t(pinned ? 'history.detail.unpin' : 'history.detail.pin')} (${formatShortcutHint('mod+p')})`}
        disabled={none}
        onClick={onTogglePin}
      />
      <Menu
        label={t('clipboard.item.actions.more')}
        icon="more"
        iconOnly
        variant="subtle"
        items={[
          ...(onManageTags
            ? [{ id: MANAGE_TAGS, label: t('history.tags.manage'), icon: 'tag' as const }]
            : []),
          { id: 'separator', kind: 'separator' as const },
          {
            id: DELETE,
            label: t('clipboard.contextMenu.delete'),
            icon: 'delete' as const,
            destructive: true,
            disabled: none,
          },
        ]}
        onAction={id => {
          if (id === MANAGE_TAGS) onManageTags?.()
          else if (id === DELETE) onDelete()
        }}
      />
    </Toolbar>
  )
}
