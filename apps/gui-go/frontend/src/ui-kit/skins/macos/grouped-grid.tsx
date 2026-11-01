import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'
import type { GridItem, GroupedGridProps, ListGroup } from '@/ui-kit/contract'
import {
  useWindowedScroll,
  revealWindowItem,
  moveWindowSelection,
} from '@/ui-kit/use-windowed-scroll'
import { MenuPanel } from './menus'
import { useAnchoredSelection } from './selection'

const GAP = 14
const PADDING = 16

type Row =
  | { kind: 'header'; group: ListGroup }
  | { kind: 'cards'; items: GridItem[]; leading?: number }

function hostOf(text: string | undefined) {
  try {
    return text ? new URL(text).host : ''
  } catch {
    return ''
  }
}

/**
 * Finder icon view of the history (design ME2): cards in day groups, as many per row as fit the
 * width at the chosen size, virtualized by rows. One tab stop with `aria-activedescendant`;
 * selection as in AppKit (click, Command-click, Shift); a selected card is accent-outlined with an
 * accent footer.
 */
export function GroupedGrid({
  label,
  groups,
  items,
  size,
  selection,
  onSelectionChange,
  onActivate,
  onDelete,
  onEndReached,
  viewport,
  contextMenu,
  footer,
}: GroupedGridProps) {
  const domId = useId()
  const gridRef = useRef<HTMLDivElement>(null)
  const virtuoso = useRef<VirtuosoHandle>(null)
  const [width, setWidth] = useState(0)
  const [menuIds, setMenuIds] = useState<string[]>([])
  const [menuOpen, setMenuOpen] = useState(false)
  const ids = items.map(item => item.id)
  const select = useAnchoredSelection(ids, selection, onSelectionChange)
  const cursor = selection[selection.length - 1]
  const cardId = (id: string) => `${domId}-card-${id}`

  useEffect(() => {
    const element = gridRef.current
    if (!element) return
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  const cardWidth = 150 + Math.round(size * 130)
  const columns = Math.max(1, Math.floor((width - 2 * PADDING + GAP) / (cardWidth + GAP)))
  // Virtuoso deduplicates endReached by data identity as well as the last index.
  // Rebuild rows only when their inputs change, not when pagination updates the parent.
  const { rows, rowOfItem, sections } = useMemo(() => {
    const rows: Row[] = []
    const rowOfItem: number[] = []
    const sections: Array<{ group: ListGroup; items: GridItem[]; start: number }> = []
    let start = 0
    for (const group of groups) {
      sections.push({ group, items: items.slice(start, start + group.count), start })
      rows.push({ kind: 'header', group })
      for (let i = start; i < start + group.count; i += columns) {
        const chunk = items.slice(i, Math.min(i + columns, start + group.count))
        for (let k = 0; k < chunk.length; k++) rowOfItem[i + k] = rows.length
        rows.push({ kind: 'cards', items: chunk })
      }
      start += group.count
    }
    return { rows, rowOfItem, sections }
  }, [groups, items, columns])

  const spacing = useWindowedScroll(gridRef, items, viewport, {
    columns,
    rowHeightRem: 16,
    key: String(size),
  })

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const index = ids.indexOf(cursor)
    if (moveWindowSelection(event.key, index, items.length, columns, viewport)) {
      event.preventDefault()
      return
    }
    const step: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -columns,
      ArrowDown: columns,
    }
    let next: number | null = null
    if (event.key in step) next = index < 0 ? 0 : index + step[event.key]
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = ids.length - 1
    if (next !== null && ids.length > 0) {
      event.preventDefault()
      const id = ids[Math.max(0, Math.min(ids.length - 1, next))]
      select.moveTo(id, event.shiftKey)
      const row = rowOfItem[ids.indexOf(id)]
      if (viewport) revealWindowItem(gridRef.current, id)
      else if (row !== undefined) virtuoso.current?.scrollIntoView({ index: row, behavior: 'auto' })
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
  const onCardMouseDown = (event: MouseEvent, id: string) => {
    event.preventDefault()
    gridRef.current?.focus({ preventScroll: true })
    select.pointerDown(event, id)
  }

  const card = (item: GridItem, leading?: number) => {
    const selected = selection.includes(item.id)
    return (
      <div
        key={item.id}
        style={leading ? { gridColumnStart: leading + 1 } : undefined}
        id={cardId(item.id)}
        className="mac-card"
        role="option"
        aria-selected={selected}
        aria-label={item.title}
        data-item-id={item.id}
        data-cursor={item.id === cursor ? '' : undefined}
        data-kind={item.kind}
        onMouseDown={event => onCardMouseDown(event, item.id)}
        onContextMenu={() => setMenuIds(selected ? selection : [item.id])}
        onDoubleClick={() => onActivate?.(item.id)}
      >
        <div className="mac-card-body">
          {item.kind === 'image' ? (
            item.media
          ) : item.kind === 'link' ? (
            <div className="mac-card-link">
              <span className="mac-card-host">{hostOf(item.text)}</span>
              <span className="mac-card-text">{item.text}</span>
            </div>
          ) : (
            <div className="mac-card-text" data-mono={item.kind === 'code' ? '' : undefined}>
              {item.text}
            </div>
          )}
          {item.badges && <span className="mac-card-badges">{item.badges}</span>}
        </div>
        <div className="mac-card-footer">
          <span className="mac-card-title">{item.title}</span>
          {item.subtitle && <span className="mac-card-subtitle">{item.subtitle}</span>}
        </div>
      </div>
    )
  }

  const renderRow = (row: Row) =>
    row.kind === 'header' ? (
      <div className="mac-glist-header" role="presentation">
        <span className="mac-glist-header-title">{row.group.title}</span>
        {row.group.detail && <span className="mac-glist-header-detail">{row.group.detail}</span>}
        <span className="mac-split-spacer" />
        {row.group.trailing && (
          <span className="mac-glist-header-trailing">{row.group.trailing}</span>
        )}
      </div>
    ) : (
      <div
        className="mac-grid-row"
        role="presentation"
        style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
      >
        {row.items.map((item, index) => card(item, index === 0 ? row.leading : undefined))}
      </div>
    )

  const grid = (
    <div
      ref={gridRef}
      className="mac-glist mac-grid"
      role="listbox"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor && ids.includes(cursor) ? cardId(cursor) : undefined}
      onKeyDown={onKeyDown}
      onContextMenuCapture={() => setMenuIds([])}
    >
      {viewport ? (
        <div
          data-history-window-scroller
          className="mac-glist-scroller"
          style={{ overflowY: 'auto', height: '100%', overflowAnchor: 'none' }}
        >
          <div aria-hidden="true" style={{ height: spacing.top }} />
          {sections.map(({ group, items: cards, start }) => (
            <div key={group.id}>
              {renderRow({ kind: 'header', group })}
              {renderRow({
                kind: 'cards',
                items: cards,
                leading: ((viewport?.offset ?? 0) + start) % columns,
              })}
            </div>
          ))}
          <div aria-hidden="true" style={{ height: spacing.bottom }} />
          {footer}
        </div>
      ) : (
        <Virtuoso
          ref={virtuoso}
          className="mac-glist-scroller"
          tabIndex={-1}
          data={rows}
          endReached={() => onEndReached?.()}
          increaseViewportBy={300}
          itemContent={(_, row) => renderRow(row)}
          components={{
            Footer: () => (footer ? <div className="mac-glist-footer">{footer}</div> : null),
          }}
        />
      )}
    </div>
  )
  if (!contextMenu) return grid
  return (
    <BaseContextMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
      <BaseContextMenu.Trigger render={<div className="mac-glist-menu-area" />}>
        {grid}
      </BaseContextMenu.Trigger>
      <MenuPanel
        items={menuOpen ? contextMenu.items(menuIds) : []}
        onSelect={id => {
          setMenuOpen(false)
          contextMenu.onAction(id, menuIds)
          gridRef.current?.focus({ preventScroll: true })
        }}
      />
    </BaseContextMenu.Root>
  )
}
