import type { RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { Menu, SearchField, Toolbar, ToolbarButton, ToolbarDivider } from '@/ui-kit'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

const MANAGE_TAGS = 'manage-tags'
const DELETE = 'delete'

interface HistoryKitToolbarProps {
  query: string
  onQueryChange: (value: string) => void
  onQuerySubmit: (value: string) => void
  searchInputRef: RefObject<HTMLInputElement | null>
  /** The entries the item commands act on: the selection. */
  targetIds: string[]
  pinned: boolean
  onTogglePin: () => void
  onDelete: () => void
  onManageTags?: () => void
}

/**
 * The trailing end of the window toolbar (design MA2): the search field, then Send, Pin and More
 * for the selection. The design's list / grid switch is not shown: the grid layout (ME2) is not
 * built yet.
 */
export function HistoryKitToolbar({
  query,
  onQueryChange,
  onQuerySubmit,
  searchInputRef,
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
      <SearchField
        label={t('history.kit.searchLabel')}
        placeholder={t('history.listSearchPlaceholder')}
        shortcutHint={formatShortcutHint('mod+f')}
        value={query}
        onValueChange={onQueryChange}
        onSubmit={onQuerySubmit}
        inputRef={searchInputRef}
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
