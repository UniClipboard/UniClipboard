import React from 'react'
import HistoryCardContextMenu from '@/components/history/HistoryCardContextMenu'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import type { QuickPanelContextMenuActions } from '@/quick-panel/types'

interface PanelItemContextMenuProps {
  /**
   * Rich item backing the menu (the menu reads `content`/`isFavorited`/`type`).
   * Absent when the row's rich data isn't ready yet — see the fallback below.
   */
  item: DisplayClipboardItem | undefined
  actions: QuickPanelContextMenuActions
  children: React.ReactNode
}

/**
 * Wraps a quick-panel row in the shared history right-click menu.
 *
 * Reuses `HistoryCardContextMenu` verbatim (copy / favorite / send / reveal /
 * delete) — the quick panel only supplies each row's rich
 * {@link DisplayClipboardItem} (looked up index-aligned from the live list) and
 * the id-based handlers. When the rich item isn't available for a row (a
 * transient index mismatch), the row renders without a menu instead of crashing
 * the menu on a missing `content`/`isFavorited`.
 */
const PanelItemContextMenu: React.FC<PanelItemContextMenuProps> = ({ item, actions, children }) => {
  if (!item) return <>{children}</>
  return (
    <HistoryCardContextMenu
      item={item}
      onCopy={actions.onCopy}
      onToggleFavorite={actions.onToggleFavorite}
      onDelete={actions.onDelete}
      // Quick-panel rows live in a flex list with a definite height; keeping the
      // default `h-full` would stretch every row's trigger to the list height,
      // leaving only the first row visible. Wrap at the row's natural height.
      triggerClassName="block w-full"
      // Rows are `role="option"` under a `role="listbox"`; keep the trigger
      // wrapper out of the a11y tree so the option stays a listbox child.
      triggerRole="presentation"
    >
      {children}
    </HistoryCardContextMenu>
  )
}

export default PanelItemContextMenu
