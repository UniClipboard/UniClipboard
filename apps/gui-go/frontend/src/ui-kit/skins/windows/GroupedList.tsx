import { Menu as FluentMenu, MenuPopover, MenuTrigger } from '@fluentui/react-components'
import { useId, useRef, useState } from 'react'
import type { KeyboardEvent, MouseEvent } from 'react'
import { GroupedVirtuoso } from 'react-virtuoso'
import type { GroupedVirtuosoHandle } from 'react-virtuoso'
import type { GroupedListProps } from '@/ui-kit/contract'
import { useWindowedScroll, revealWindowItem } from '@/ui-kit/use-windowed-scroll'
import { useGroupedListStyles } from './grouped-list-styles'
import { GroupedListFooter } from './GroupedListFooter'
import { GroupedListRow } from './GroupedListRow'
import { listStep, selectRange } from './list-keys'
import { useMenuStyles } from './menu-styles'
import { MenuEntries } from './MenuEntries'

const footerComponents = { Footer: GroupedListFooter }

/**
 * WinUI ListView in Extended selection mode with grouped, sticky headers, virtualized. The list is
 * the one tab stop and names the current row with `aria-activedescendant`, so keyboard focus
 * survives rows scrolling out of the DOM. Click selects one row, Ctrl+click toggles a row,
 * Shift+click and Shift+arrow select a range from the anchor, Ctrl+A selects every loaded row,
 * Enter activates and Delete reports the selection. The context menu opens at the pointer and acts
 * on the selection, or on the right-clicked row when it is not selected.
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
  const styles = useGroupedListStyles()
  const menuStyles = useMenuStyles()
  const domId = useId()
  const listRef = useRef<HTMLDivElement>(null)
  const virtuoso = useRef<GroupedVirtuosoHandle>(null)
  // The fixed end of a Shift range; the moving end is the last selected id.
  const anchor = useRef<string | null>(null)
  // Ids the next context menu acts on, set by the contextmenu event before the menu opens.
  const pendingMenuIds = useRef<string[]>([])
  const [menuIds, setMenuIds] = useState<string[] | null>(null)
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

  const selectOne = (id: string) => {
    anchor.current = id
    onSelectionChange([id])
  }

  useWindowedScroll(listRef, items, viewport)

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const index = items.findIndex(item => item.id === cursor)
    const next = listStep(event.key, index, items.length)
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      if (event.shiftKey && anchor.current)
        onSelectionChange(selectRange(items, anchor.current, id))
      else selectOne(id)
      reveal(id)
    } else if (event.ctrlKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      onSelectionChange(items.map(item => item.id))
    } else if (event.key === 'Enter' && cursor) {
      event.preventDefault()
      onActivate?.(cursor)
    } else if (event.key === 'Delete' && selection.length > 0 && onDelete) {
      event.preventDefault()
      onDelete(selection)
    }
  }

  const onRowMouseDown = (event: MouseEvent, id: string) => {
    // Keep focus on the listbox: the default action would focus Virtuoso's scroller instead (and
    // Shift+click would select text).
    event.preventDefault()
    listRef.current?.focus({ preventScroll: true })
    const selected = selection.includes(id)
    if (event.button === 2) {
      // A secondary click keeps a selection that contains the row (the menu acts on all of it).
      if (!selected) selectOne(id)
      return
    }
    if (event.button !== 0) return
    if (event.ctrlKey) {
      anchor.current = id
      onSelectionChange(selected ? selection.filter(s => s !== id) : [...selection, id])
    } else if (event.shiftKey && anchor.current) {
      onSelectionChange(selectRange(items, anchor.current, id))
    } else {
      selectOne(id)
    }
  }

  const groupStarts: number[] = []
  groups.reduce((start, group) => {
    groupStarts.push(start)
    return start + group.count
  }, 0)
  const renderGroup = (index: number) => {
    const group = groups[index]
    return (
      <div className={styles.header} role="presentation">
        <span className={styles.headerTitle}>{group.title}</span>
        {group.detail ? <span className={styles.headerDetail}>{group.detail}</span> : null}
        {group.trailing ? <span className={styles.headerTrailing}>{group.trailing}</span> : null}
      </div>
    )
  }
  const renderItem = (index: number) => {
    const item = items[index]
    if (!item) return null
    const selected = selection.includes(item.id)
    return (
      <GroupedListRow
        item={item}
        domId={rowId(item.id)}
        selected={selected}
        multi={selection.length > 1}
        cursor={item.id === cursor}
        onMouseDown={event => onRowMouseDown(event, item.id)}
        onContextMenu={() => {
          pendingMenuIds.current = selected ? selection : [item.id]
        }}
        onDoubleClick={() => onActivate?.(item.id)}
      />
    )
  }

  const list = (
    <div
      ref={listRef}
      className={styles.list}
      role="listbox"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor ? rowId(cursor) : undefined}
      onKeyDown={onKeyDown}
      // The context-menu key (target: the list) acts on the selection; a right-click outside any
      // row has nothing to act on. A row's own handler runs after this one.
      onContextMenuCapture={event => {
        pendingMenuIds.current = event.target === event.currentTarget ? selection : []
      }}
    >
      {viewport ? (
        <div
          data-history-window-scroller
          className={styles.scroller}
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
          className={styles.scroller}
          // The listbox is the one tab stop; Virtuoso's scroller is focusable by default.
          tabIndex={-1}
          groupCounts={groups.map(group => group.count)}
          endReached={() => onEndReached?.()}
          increaseViewportBy={240}
          context={{ footer }}
          components={footerComponents}
          groupContent={renderGroup}
          itemContent={renderItem}
        />
      )}
    </div>
  )

  if (!contextMenu) return list
  return (
    <FluentMenu
      openOnContext
      open={menuIds !== null}
      onOpenChange={(_, data) => {
        if (!data.open) {
          setMenuIds(null)
          // The trigger is a wrapper, not focusable: Escape or a click outside would leave focus on
          // <body>. Like a WinUI ListView, focus goes back to the list.
          listRef.current?.focus({ preventScroll: true })
        } else if (pendingMenuIds.current.length > 0) setMenuIds(pendingMenuIds.current)
      }}
    >
      <MenuTrigger disableButtonEnhancement>
        <div className={styles.menuArea}>{list}</div>
      </MenuTrigger>
      <MenuPopover className={menuStyles.popover}>
        <MenuEntries
          items={menuIds ? contextMenu.items(menuIds) : []}
          onAction={id => {
            contextMenu.onAction(id, menuIds ?? [])
            listRef.current?.focus({ preventScroll: true })
          }}
        />
      </MenuPopover>
    </FluentMenu>
  )
}
