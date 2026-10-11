import {
  Menu as FluentMenu,
  MenuPopover,
  MenuTrigger,
  makeStyles,
  mergeClasses,
  tokens,
} from '@fluentui/react-components'
import { useId, useRef, useState } from 'react'
import type { KeyboardEvent, MouseEvent } from 'react'
import type { TableProps } from '@/ui-kit/contract'
import { listStep, selectRange } from './list-keys'
import { useMenuStyles } from './menu-styles'
import { MenuEntries } from './MenuEntries'

// Board WC3: a WinUI ListView with column headers on the card layer: 52 px rows, the subtle
// selection fill with the 3 x 16 accent pill, right-aligned numeric columns.
const useStyles = makeStyles({
  frame: {
    borderRadius: tokens.borderRadiusXLarge,
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    backgroundColor: 'var(--w11-card)',
    overflow: 'hidden',
  },
  grid: {
    display: 'flex',
    flexDirection: 'column',
    outline: 'none',
    paddingBottom: '0.25rem',
    '&:focus-visible [data-cursor]': {
      outline: `${tokens.strokeWidthThick} solid ${tokens.colorStrokeFocus2}`,
      outlineOffset: '-2px',
    },
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    minHeight: '2.75rem',
    paddingInline: '1rem 0.5rem',
    borderBottom: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    fontSize: tokens.fontSizeBase300,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorNeutralForeground2,
  },
  row: {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
    minHeight: '3.25rem',
    marginInline: '0.25rem',
    paddingInline: '0.75rem 0.25rem',
    borderRadius: tokens.borderRadiusMedium,
    fontSize: tokens.fontSizeBase300,
    cursor: 'default',
    userSelect: 'none',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    '&[aria-selected="true"]': { backgroundColor: 'var(--w11-selected)' },
    '&[aria-selected="true"]::before': {
      content: '""',
      position: 'absolute',
      insetInlineStart: 0,
      top: 'calc(50% - 0.5rem)',
      width: '0.1875rem',
      height: '1rem',
      borderRadius: '0.125rem',
      backgroundColor: tokens.colorBrandBackground,
    },
  },
  cell: {
    minWidth: 0,
    flex: '0 0 auto',
    paddingInline: '0.5rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
  },
  fill: { flex: '1 1 auto' },
  start: { justifyContent: 'flex-start' },
  end: { justifyContent: 'flex-end', minWidth: '4.5rem' },
  center: { justifyContent: 'center', minWidth: '6rem' },
  empty: { padding: '2rem 1rem', textAlign: 'center', color: tokens.colorNeutralForeground2 },
})

/**
 * Keyboard as GroupedList: the grid is one tab stop naming the current row with
 * aria-activedescendant; Up/Down/Home/End move, Shift extends, Ctrl+A selects all, Delete reports
 * the selection, Shift+F10 or the menu key opens the context menu. Controls inside cells (switches,
 * row menus) stay reachable with Tab. Click selects, Ctrl+click toggles, Shift+click selects a range.
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
  const styles = useStyles()
  const menuStyles = useMenuStyles()
  const domId = useId()
  const gridRef = useRef<HTMLDivElement>(null)
  const anchor = useRef<string | null>(null)
  const pendingMenuIds = useRef<string[]>([])
  const [menuIds, setMenuIds] = useState<string[] | null>(null)
  const cursor = selection[selection.length - 1]
  const rowId = (id: string) => `${domId}-row-${id}`
  const columnClass = (column: (typeof columns)[number]) =>
    mergeClasses(styles.cell, column.fill && styles.fill, styles[column.align ?? 'start'])

  const selectOne = (id: string) => {
    anchor.current = id
    onSelectionChange([id])
  }
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    const index = rows.findIndex(row => row.id === cursor)
    const next = listStep(event.key, index, rows.length)
    if (next !== null) {
      event.preventDefault()
      const id = rows[next].id
      if (event.shiftKey && anchor.current) onSelectionChange(selectRange(rows, anchor.current, id))
      else selectOne(id)
      document.getElementById(rowId(id))?.scrollIntoView({ block: 'nearest' })
    } else if (event.ctrlKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      onSelectionChange(rows.map(row => row.id))
    } else if (event.key === 'Delete' && selection.length > 0 && onDelete) {
      event.preventDefault()
      onDelete(selection)
    }
  }
  const onRowMouseDown = (event: MouseEvent, id: string) => {
    // Clicks inside a cell control (switch, menu button) act on the control only.
    if ((event.target as HTMLElement).closest('button, input, [role="switch"]')) return
    gridRef.current?.focus({ preventScroll: true })
    const selected = selection.includes(id)
    if (event.button === 2) {
      if (!selected) selectOne(id)
      return
    }
    if (event.button !== 0) return
    event.preventDefault()
    if (event.ctrlKey) {
      anchor.current = id
      onSelectionChange(selected ? selection.filter(s => s !== id) : [...selection, id])
    } else if (event.shiftKey && anchor.current) {
      onSelectionChange(selectRange(rows, anchor.current, id))
    } else {
      selectOne(id)
    }
  }

  const grid = (
    <div
      ref={gridRef}
      className={styles.grid}
      role="grid"
      tabIndex={0}
      aria-label={label}
      aria-multiselectable
      aria-rowcount={rows.length}
      aria-activedescendant={cursor ? rowId(cursor) : undefined}
      onKeyDown={onKeyDown}
      onContextMenuCapture={event => {
        pendingMenuIds.current = event.target === event.currentTarget ? selection : []
      }}
    >
      <div className={styles.header} role="row">
        {columns.map(column => (
          <span key={column.id} role="columnheader" className={columnClass(column)}>
            {column.label}
          </span>
        ))}
      </div>
      {rows.length === 0 && empty ? (
        <div className={styles.empty} role="row">
          <span role="gridcell">{empty}</span>
        </div>
      ) : null}
      {rows.map(row => {
        const selected = selection.includes(row.id)
        return (
          <div
            key={row.id}
            id={rowId(row.id)}
            role="row"
            aria-selected={selected}
            data-row-id={row.id}
            data-cursor={row.id === cursor ? '' : undefined}
            className={styles.row}
            onMouseDown={event => onRowMouseDown(event, row.id)}
            onContextMenu={() => {
              pendingMenuIds.current = selected ? selection : [row.id]
            }}
          >
            {columns.map(column => (
              <span key={column.id} role="gridcell" className={columnClass(column)}>
                {row.cells[column.id]}
              </span>
            ))}
          </div>
        )
      })}
    </div>
  )

  const framed = <div className={styles.frame}>{grid}</div>
  if (!contextMenu) return framed
  return (
    <FluentMenu
      openOnContext
      open={menuIds !== null}
      onOpenChange={(_, data) => {
        if (!data.open) {
          setMenuIds(null)
          gridRef.current?.focus({ preventScroll: true })
        } else if (pendingMenuIds.current.length > 0) setMenuIds(pendingMenuIds.current)
      }}
    >
      <MenuTrigger disableButtonEnhancement>{framed}</MenuTrigger>
      <MenuPopover className={menuStyles.popover}>
        <MenuEntries
          items={menuIds ? contextMenu.items(menuIds) : []}
          onAction={id => {
            contextMenu.onAction(id, menuIds ?? [])
            gridRef.current?.focus({ preventScroll: true })
          }}
        />
      </MenuPopover>
    </FluentMenu>
  )
}
