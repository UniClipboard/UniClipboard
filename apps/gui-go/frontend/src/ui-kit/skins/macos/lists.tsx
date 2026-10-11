import { useRef, useState, type KeyboardEvent } from 'react'
import type { ItemListProps, SidebarNavProps } from '@/ui-kit/contract'
import { glyph } from './glyph'
import { Icon } from './icons'
import { listStep } from './list-keys'
import { Checkbox } from './parts'
import { swatchColor } from './swatch'

/** Moves DOM focus to the item at `index` inside a roving-tabindex container. */
function focusItem(container: HTMLElement | null, selector: string, index: number) {
  container?.querySelectorAll<HTMLElement>(selector)[index]?.focus()
}

/**
 * NSTableView-style list with one tab stop (roving focus on the rows). Single selection: arrows move
 * the selection. With `multiSelect` (checkbox mode) arrows move focus, Space toggles the focused row,
 * Shift+arrow extends and Command-click adds a row; a plain click selects only that row, as in AppKit.
 * Return or double click activates; Delete and Backspace report the selection. Selection turns gray
 * when the window is not key.
 */
// A shared empty default, so an omitted prop does not create a new array on every render.
const noSelection: string[] = []

export function ItemList({
  label,
  items,
  selection,
  defaultSelection = noSelection,
  onSelectionChange,
  multiSelect,
  onActivate,
  onDelete,
}: ItemListProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const [own, setOwn] = useState(defaultSelection)
  const [focusedId, setFocusedId] = useState<string | undefined>()
  const selected = selection ?? own
  const tabStop =
    items.find(item => item.id === focusedId)?.id ??
    items.find(item => selected.includes(item.id))?.id ??
    items[0]?.id
  const select = (ids: string[]) => {
    if (selection === undefined) setOwn(ids)
    onSelectionChange?.(ids)
  }
  const toggle = (id: string) =>
    select(selected.includes(id) ? selected.filter(s => s !== id) : [...selected, id])

  const onKeyDown = (event: KeyboardEvent) => {
    // Only keys on a row itself: controls inside a row (a removable tag) keep their own keys, and
    // a Delete there must not delete the selected items.
    if (!(event.target instanceof HTMLElement) || event.target.getAttribute('role') !== 'option')
      return
    const index = items.findIndex(item => item.id === tabStop)
    const next = listStep(event.key, index, items.length)
    if (next !== null) {
      event.preventDefault()
      const id = items[next].id
      if (!multiSelect) select([id])
      else if (event.shiftKey) select([...new Set([...selected, id])])
      focusItem(listRef.current, '[role="option"]', next)
    } else if (event.key === ' ' && tabStop) {
      event.preventDefault()
      toggle(tabStop)
    } else if (event.key === 'Enter' && tabStop) {
      event.preventDefault()
      onActivate?.(tabStop)
    } else if (
      (event.key === 'Backspace' || event.key === 'Delete') &&
      selected.length > 0 &&
      onDelete
    ) {
      event.preventDefault()
      onDelete(selected)
    }
  }

  return (
    <div
      ref={listRef}
      className="mac-item-list"
      role="listbox"
      aria-label={label}
      aria-multiselectable={multiSelect || undefined}
      onKeyDown={onKeyDown}
    >
      {items.map(item => {
        const isSelected = selected.includes(item.id)
        return (
          <div
            key={item.id}
            className="mac-row"
            role="option"
            aria-label={item.title}
            aria-selected={isSelected}
            tabIndex={item.id === tabStop ? 0 : -1}
            onFocus={() => {
              setFocusedId(item.id)
              // Single selection follows focus, as in NSTableView.
              if (!multiSelect && !isSelected) select([item.id])
            }}
            onMouseDown={event => {
              if (multiSelect && event.metaKey) toggle(item.id)
              else if (!(event.button === 2 && isSelected)) select([item.id])
            }}
            onDoubleClick={() => onActivate?.(item.id)}
          >
            {multiSelect && (
              <Checkbox
                checked={isSelected}
                variant={isSelected ? 'on-selection' : 'default'}
                tabIndex={-1}
                aria-label={item.title}
                onCheckedChange={() => toggle(item.id)}
              />
            )}
            {item.leading}
            <span className="mac-row-text">
              <span className="mac-row-title" data-mono={item.monospace ? '' : undefined}>
                {item.title}
              </span>
              {item.subtitle && <span className="mac-row-meta">{item.subtitle}</span>}
            </span>
            {item.trailing && <span className="mac-row-trailing">{item.trailing}</span>}
          </div>
        )
      })}
    </div>
  )
}

/**
 * Source-list sidebar as a navigation landmark: one tab stop, arrows move focus, Return or Space
 * selects. Selection is a light rounded fill with an accent symbol, not a blue row.
 */
export function SidebarNav({
  label,
  sections,
  value,
  defaultValue,
  onValueChange,
}: SidebarNavProps) {
  const navRef = useRef<HTMLElement>(null)
  const flat = sections.flatMap(section => section.items)
  const [own, setOwn] = useState(defaultValue)
  const [focusedId, setFocusedId] = useState<string | undefined>()
  const current = value ?? own
  const tabStop = flat.find(item => item.id === focusedId)?.id ?? current ?? flat[0]?.id
  const choose = (id: string) => {
    if (value === undefined) setOwn(id)
    onValueChange?.(id)
  }
  const onKeyDown = (event: KeyboardEvent) => {
    const next = listStep(
      event.key,
      flat.findIndex(item => item.id === tabStop),
      flat.length
    )
    if (next !== null) {
      event.preventDefault()
      focusItem(navRef.current, '.mac-side-item', next)
    }
  }
  return (
    <nav ref={navRef} className="mac-sidebar-nav" aria-label={label} onKeyDown={onKeyDown}>
      {sections.map(section => (
        <div key={section.id} className="mac-sidebar-group">
          {section.title && <div className="mac-side-heading">{section.title}</div>}
          {section.items.map(item => (
            <button
              key={item.id}
              type="button"
              className="mac-side-item"
              aria-current={item.id === current ? 'page' : undefined}
              tabIndex={item.id === tabStop ? 0 : -1}
              onFocus={() => setFocusedId(item.id)}
              onClick={() => choose(item.id)}
            >
              <span className="mac-side-icon">
                {item.icon && <Icon name={glyph(item.icon)} size={15} />}
                {item.swatch && (
                  <span className="mac-dot-tag" style={{ background: swatchColor[item.swatch] }} />
                )}
              </span>
              <span className="mac-side-label">{item.label}</span>
              {item.count && <span className="mac-side-count">{item.count}</span>}
            </button>
          ))}
        </div>
      ))}
    </nav>
  )
}
