import { useTranslation } from 'react-i18next'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { Menu, SegmentedControl, Slider, Toolbar, ToolbarButton, ToolbarDivider } from '@/ui-kit'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

const MANAGE_TAGS = 'manage-tags'
const DELETE = 'delete'

interface HistoryKitToolbarProps {
  view: 'list' | 'grid'
  onViewChange: (view: 'list' | 'grid') => void
  /** Card size of the grid, 0 to 1. */
  gridSize: number
  onGridSizeChange: (size: number) => void
  /** The entries the item commands act on: the selection. */
  targetIds: string[]
  pinned: boolean
  onTogglePin: () => void
  onDelete: () => void
  onManageTags?: () => void
}

/**
 * The trailing toolbar group after the search field (design MA2/ME2): the list / grid switch (with
 * the card size slider in the grid), then Send, Pin and More for the selection.
 */
export function HistoryKitToolbar({
  view,
  onViewChange,
  gridSize,
  onGridSizeChange,
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
      {view === 'grid' && (
        <Slider
          label={t('history.kit.cardSize')}
          value={gridSize}
          onValueChange={onGridSizeChange}
          min={0}
          max={1}
          step={0.05}
        />
      )}
      <SegmentedControl
        label={t('history.kit.view')}
        value={view}
        onValueChange={value => onViewChange(value as 'list' | 'grid')}
        items={[
          { value: 'list', label: t('history.kit.viewList'), icon: 'list', iconOnly: true },
          { value: 'grid', label: t('history.kit.viewGrid'), icon: 'grid', iconOnly: true },
        ]}
      />
      <ToolbarDivider />
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
