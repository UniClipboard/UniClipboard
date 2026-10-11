import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { useId, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import type { TableProps } from '@/ui-kit/contract'
import { listStep } from './list-keys'
import { MenuPanel } from './menus'

/**
 * NSTableView with alternating rows (design MC3): a header row, one tab stop that names the current
 * row with `aria-activedescendant`, AppKit selection (click, Command-click, Shift-click, arrows),
 * and a context menu for the selection or the right-clicked row.
 */
export function Table({
  label,
  columns,
  rows,
  selection,
  onSelectionChange,
  onDelete,
  contextMenu,
  empty,
}: TableProps) {
  const domId = useId()
  const gridRef = useRef<HTMLDivElement>(null)
  const anchor = useRef<string | null>(null)
  const [menuIds, setMenuIds] = useState<string[]>([])
  const [menuOpen, setMenuOpen] = useState(false)
  const cursor = selection[selection.length - 1]
  const rowId = (id: string) => `${domId}-row-${id}`
  const range = (from: string, to: string) => {
    const a = rows.findIndex(row => row.id === from)
    const b = rows.findIndex(row => row.id === to)
    if (a < 0 || b < 0) return [to]
    const [start, end] = a <= b ? [a, b] : [b, a]
    return rows.slice(start, end + 1).map(row => row.id)
  }
  const template = columns.map(column => (column.fill ? 'minmax(0, 1fr)' : 'auto')).join(' ')

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const next = listStep(
      event.key,
      rows.findIndex(row => row.id === cursor),
      rows.length
    )
    if (next !== null) {
      event.preventDefault()
      const id = rows[next].id
      if (event.shiftKey && anchor.current) {
        onSelectionChange([...range(anchor.current, id).filter(i => i !== id), id])
      } else {
        anchor.current = id
        onSelectionChange([id])
      }
      document.getElementById(rowId(id))?.scrollIntoView({ block: 'nearest' })
    } else if (event.metaKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      onSelectionChange(rows.map(row => row.id))
    } else if ((event.key === 'Backspace' || event.key === 'Delete') && selection.length > 0) {
      if (!onDelete) return
      event.preventDefault()
      onDelete(selection)
    }
  }
  const onRowMouseDown = (event: MouseEvent, id: string) => {
    // Controls inside a row (a switch, a field) keep their own pointer behaviour.
    if ((event.target as HTMLElement).closest('button, input, [role="switch"]')) return
    gridRef.current?.focus({ preventScroll: true })
    const selected = selection.includes(id)
    if (event.button === 2 || (event.button === 0 && event.ctrlKey)) {
      if (!selected) {
        anchor.current = id
        onSelectionChange([id])
      }
      return
    }
    if (event.button !== 0) return
    if (event.metaKey) {
      anchor.current = id
      onSelectionChange(selected ? selection.filter(s => s !== id) : [...selection, id])
    } else if (event.shiftKey && anchor.current) {
      onSelectionChange([...range(anchor.current, id).filter(i => i !== id), id])
    } else {
      anchor.current = id
      onSelectionChange([id])
    }
  }

  const grid = (
    <div
      ref={gridRef}
      className="mac-table"
      role="grid"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-activedescendant={cursor ? rowId(cursor) : undefined}
      onKeyDown={onKeyDown}
      onContextMenuCapture={() => setMenuIds([])}
      style={{ gridTemplateColumns: template }}
    >
      <div className="mac-table-row mac-table-head" role="row">
        {columns.map(column => (
          <span key={column.id} role="columnheader" data-align={column.align}>
            {column.label}
          </span>
        ))}
      </div>
      {rows.map(row => (
        <div
          key={row.id}
          id={rowId(row.id)}
          className="mac-table-row"
          role="row"
          aria-selected={selection.includes(row.id)}
          data-cursor={row.id === cursor ? '' : undefined}
          onMouseDown={event => onRowMouseDown(event, row.id)}
          onContextMenu={() => setMenuIds(selection.includes(row.id) ? selection : [row.id])}
        >
          {columns.map(column => (
            <span key={column.id} role="gridcell" data-align={column.align}>
              {row.cells[column.id]}
            </span>
          ))}
        </div>
      ))}
      {rows.length === 0 && empty && <div className="mac-table-empty">{empty}</div>}
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
