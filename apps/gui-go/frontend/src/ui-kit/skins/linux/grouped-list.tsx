import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { useId, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { GroupedVirtuoso, type GroupedVirtuosoHandle } from 'react-virtuoso'
import type { GroupedListProps } from '@/ui-kit/contract'
import { useSinglePaneLayout } from '@/ui-kit/skins/linux/adaptive-layout'
import { Highlighted } from './highlighted'
import { listStep } from './list-keys'
import { MenuPanel } from './menus'
import { useAnchoredSelection } from './selection'

/**
 * Source list with day groups, virtualized. The list itself is the one tab stop and
 * names the current row with `aria-activedescendant`, so keyboard focus survives rows scrolling out
 * of the DOM. Click selects one row, Ctrl-click toggles a row, Shift-click and Shift-arrow
 * select a range from the anchor, Ctrl+A selects every loaded row; selected rows are accent-tinted
 * with no checkboxes.
 */
export function GroupedList({
  label,
  groups,
  items,
  selection,
  onSelectionChange,
  onActivate,
  onInspect,
  onDelete,
  onEndReached,
  contextMenu,
  footer,
}: GroupedListProps) {
  const domId = useId()
  const singlePane = useSinglePaneLayout()
  const listRef = useRef<HTMLDivElement>(null)
  const virtuoso = useRef<GroupedVirtuosoHandle>(null)
  const ids = items.map(item => item.id)
  const select = useAnchoredSelection(ids, selection, onSelectionChange)
  const [menuIds, setMenuIds] = useState<string[]>([])
  const [menuOpen, setMenuOpen] = useState(false)
  const cursor = selection[selection.length - 1]
  const rowId = (id: string) => `${domId}-row-${id}`

  const reveal = (id: string) => {
    const index = items.findIndex(item => item.id === id)
    if (index >= 0) virtuoso.current?.scrollIntoView({ index, behavior: 'auto' })
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const index = items.findIndex(item => item.id === cursor)
    const next = listStep(event.key, index, items.length)
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      select.moveTo(id, event.shiftKey)
      reveal(id)
    } else if (event.ctrlKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      select.selectAll()
    } else if (event.key === 'Enter' && cursor) {
      event.preventDefault()
      if (singlePane && onInspect) onInspect(cursor)
      else onActivate?.(cursor)
    } else if ((event.key === 'Backspace' || event.key === 'Delete') && selection.length > 0) {
      if (!onDelete) return
      event.preventDefault()
      onDelete(selection)
    }
  }

  const onRowMouseDown = (event: MouseEvent, id: string) => {
    // Keep focus on the list (not the scroller) and keep text from being selected.
    event.preventDefault()
    listRef.current?.focus({ preventScroll: true })
    select.pointerDown(event, id)
  }

  const groupStarts: number[] = []
  groups.reduce((start, group) => {
    groupStarts.push(start)
    return start + group.count
  }, 0)

  const list = (
    <div
      ref={listRef}
      className="adw-glist"
      role="listbox"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor ? rowId(cursor) : undefined}
      onKeyDown={onKeyDown}
      // A right-click outside any row has nothing to act on; a row sets its ids after this.
      onContextMenuCapture={() => setMenuIds([])}
    >
      <GroupedVirtuoso
        ref={virtuoso}
        className="adw-glist-scroller"
        // The list is the one tab stop; the scroller must not take focus from it.
        tabIndex={-1}
        groupCounts={groups.map(group => group.count)}
        endReached={() => onEndReached?.()}
        increaseViewportBy={240}
        groupContent={index => {
          const group = groups[index]
          return (
            <div className="adw-glist-header" role="presentation">
              <span className="adw-glist-header-title">{group.title}</span>
              {group.detail && <span className="adw-glist-header-detail">{group.detail}</span>}
              <span className="adw-split-spacer" />
              {group.trailing && (
                <span className="adw-glist-header-trailing">{group.trailing}</span>
              )}
            </div>
          )
        }}
        itemContent={(index, groupIndex) => {
          const item = items[index]
          if (!item) return null
          const isSelected = selection.includes(item.id)
          // Rows of a day group form one boxed list: the cell draws its first and last edge.
          const first = index === groupStarts[groupIndex]
          const last = index === groupStarts[groupIndex] + groups[groupIndex].count - 1
          return (
            <div
              className="adw-glist-cell"
              data-first={first ? '' : undefined}
              data-last={last ? '' : undefined}
            >
              <div
                id={rowId(item.id)}
                className="adw-row"
                role="option"
                aria-selected={isSelected}
                aria-label={item.title}
                data-item-id={item.id}
                data-cursor={item.id === cursor ? '' : undefined}
                onMouseDown={event => onRowMouseDown(event, item.id)}
                onClick={event => {
                  if (singlePane && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
                    onInspect?.(item.id)
                  }
                }}
                onContextMenu={() => {
                  setMenuIds(isSelected ? selection : [item.id])
                }}
                onDoubleClick={() => {
                  if (!singlePane || !onInspect) onActivate?.(item.id)
                }}
              >
                {item.leading}
                <span className="adw-row-text">
                  <span className="adw-row-title" data-mono={item.monospace ? '' : undefined}>
                    <Highlighted text={item.title} terms={item.highlight} />
                  </span>
                  {item.subtitle && <span className="adw-row-meta">{item.subtitle}</span>}
                </span>
                {item.trailing && <span className="adw-row-trailing">{item.trailing}</span>}
              </div>
            </div>
          )
        }}
        components={{
          Footer: () => (footer ? <div className="adw-glist-footer">{footer}</div> : null),
        }}
      />
    </div>
  )

  if (!contextMenu) return list
  return (
    <BaseContextMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
      <BaseContextMenu.Trigger render={<div className="adw-glist-menu-area" />}>
        {list}
      </BaseContextMenu.Trigger>
      <MenuPanel
        items={menuOpen ? contextMenu.items(menuIds) : []}
        onSelect={id => {
          setMenuOpen(false)
          contextMenu.onAction(id, menuIds)
          listRef.current?.focus({ preventScroll: true })
        }}
      />
    </BaseContextMenu.Root>
  )
}
