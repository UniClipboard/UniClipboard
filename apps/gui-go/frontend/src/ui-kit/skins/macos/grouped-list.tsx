import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { useId, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { GroupedVirtuoso, type GroupedVirtuosoHandle } from 'react-virtuoso'
import type { GroupedListProps } from '@/ui-kit/contract'
import { useWindowedScroll, revealWindowItem } from '@/ui-kit/use-windowed-scroll'
import { Highlighted } from './highlighted'
import { listStep } from './list-keys'
import { MenuPanel } from './menus'
import { useAnchoredSelection } from './selection'

/**
 * NSTableView source list with day groups, virtualized. The list itself is the one tab stop and
 * names the current row with `aria-activedescendant`, so keyboard focus survives rows scrolling out
 * of the DOM. Selection follows AppKit: click selects one row, Command-click toggles a row,
 * Shift-click and Shift-arrow select a range from the anchor, Command-A selects every loaded row;
 * selected rows are accent-filled with no checkboxes, and turn gray while the window is inactive.
 */
export function GroupedList({
  label,
  groups,
  items,
  selection,
  onSelectionChange,
  onActivate,
  onDelete,
  onEndReached,
  viewport,
  contextMenu,
  footer,
}: GroupedListProps) {
  const domId = useId()
  const listRef = useRef<HTMLDivElement>(null)
  const virtuoso = useRef<GroupedVirtuosoHandle>(null)
  const ids = items.map(item => item.id)
  const select = useAnchoredSelection(ids, selection, onSelectionChange)
  const [menuIds, setMenuIds] = useState<string[]>([])
  const [menuOpen, setMenuOpen] = useState(false)
  const cursor = selection[selection.length - 1]
  const rowId = (id: string) => `${domId}-row-${id}`

  const reveal = (id: string) => {
    if (viewport) {
      revealWindowItem(listRef.current, id)
      return
    }
    const index = items.findIndex(item => item.id === id)
    if (index >= 0) virtuoso.current?.scrollIntoView({ index: index, behavior: 'auto' })
  }

  useWindowedScroll(listRef, items, viewport)

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const index = items.findIndex(item => item.id === cursor)
    const next = listStep(event.key, index, items.length)
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      select.moveTo(id, event.shiftKey)
      reveal(id)
    } else if (event.metaKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      select.selectAll()
    } else if (event.key === 'Enter' && cursor) {
      event.preventDefault()
      onActivate?.(cursor)
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

  const renderGroup = (index: number) => {
    const group = groups[index]
    return (
      <div className="mac-glist-header" role="presentation">
        <span className="mac-glist-header-title">{group.title}</span>
        {group.detail && <span className="mac-glist-header-detail">{group.detail}</span>}
        <span className="mac-split-spacer" />
        {group.trailing && <span className="mac-glist-header-trailing">{group.trailing}</span>}
      </div>
    )
  }
  const renderItem = (index: number) => {
    const item = items[index]
    if (!item) return null
    const isSelected = selection.includes(item.id)
    return (
      <div className="mac-glist-cell">
        <div
          id={rowId(item.id)}
          className="mac-row"
          role="option"
          aria-selected={isSelected}
          aria-label={item.title}
          data-item-id={item.id}
          data-cursor={item.id === cursor ? '' : undefined}
          onMouseDown={event => onRowMouseDown(event, item.id)}
          onContextMenu={() => {
            setMenuIds(isSelected ? selection : [item.id])
          }}
          onDoubleClick={() => onActivate?.(item.id)}
        >
          {item.leading}
          <span className="mac-row-text">
            <span className="mac-row-title" data-mono={item.monospace ? '' : undefined}>
              <Highlighted text={item.title} terms={item.highlight} />
            </span>
            {item.subtitle && <span className="mac-row-meta">{item.subtitle}</span>}
          </span>
          {item.trailing && <span className="mac-row-trailing">{item.trailing}</span>}
        </div>
      </div>
    )
  }

  const list = (
    <div
      ref={listRef}
      className="mac-glist"
      role="listbox"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor ? rowId(cursor) : undefined}
      onKeyDown={onKeyDown}
      // A right-click outside any row has nothing to act on; a row sets its ids after this.
      onContextMenuCapture={() => setMenuIds([])}
    >
      {viewport ? (
        <div
          data-history-window-scroller
          className={'mac-glist-scroller'}
          style={{ overflowY: 'auto', height: '100%', overflowAnchor: 'none' }}
        >
          {groups.map((group, groupIndex) => (
            <div key={groupIndex}>
              {renderGroup(groupIndex)}
              {Array.from({ length: group.count }, (_, local) => (
                <div key={items[groupStarts[groupIndex] + local].id}>
                  {renderItem(groupStarts[groupIndex] + local)}
                </div>
              ))}
            </div>
          ))}
          {footer}
        </div>
      ) : (
        <GroupedVirtuoso
          ref={virtuoso}
          className="mac-glist-scroller"
          // The list is the one tab stop; the scroller must not take focus from it.
          tabIndex={-1}
          groupCounts={groups.map(group => group.count)}
          endReached={() => onEndReached?.()}
          increaseViewportBy={240}
          groupContent={renderGroup}
          itemContent={renderItem}
          components={{
            Footer: () => (footer ? <div className="mac-glist-footer">{footer}</div> : null),
          }}
        />
      )}
    </div>
  )

  if (!contextMenu) return list
  return (
    <BaseContextMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
      <BaseContextMenu.Trigger render={<div className="mac-glist-menu-area" />}>
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
