import { useRef, useState, type KeyboardEvent } from 'react'
import type { ItemListProps, SidebarNavItem, SidebarNavProps } from '@/ui-kit/contract'
import { WithHelpTag } from './controls'
import { glyph } from './glyph'
import { Icon } from './icons'
import { listStep } from './list-keys'
import { Checkbox, PushButton } from './parts'
import { swatchColor } from './swatch'

/** Moves DOM focus to the item at `index` inside a roving-tabindex container. */
function focusItem(container: HTMLElement | null, selector: string, index: number) {
  container?.querySelectorAll<HTMLElement>(selector)[index]?.focus()
}

/**
 * Selectable list with one tab stop (roving focus on the rows). Single selection: arrows move the
 * selection. With `multiSelect` (checkbox mode) arrows move focus, Space toggles the focused row,
 * Shift+arrow extends and Ctrl-click adds a row; a plain click selects only that row. Enter or
 * double click activates; Delete and Backspace report the selection.
 */
// A shared empty default, so an omitted prop does not create a new array on every render.
const noSelection: string[] = []
const noItems: SidebarNavItem[] = []

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
      className="adw-item-list"
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
            className="adw-row"
            role="option"
            aria-label={item.title}
            aria-selected={isSelected}
            tabIndex={item.id === tabStop ? 0 : -1}
            onFocus={() => {
              setFocusedId(item.id)
              // Single selection follows focus.
              if (!multiSelect && !isSelected) select([item.id])
            }}
            onMouseDown={event => {
              if (multiSelect && event.ctrlKey) toggle(item.id)
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
            <span className="adw-row-text">
              <span className="adw-row-title" data-mono={item.monospace ? '' : undefined}>
                {item.title}
              </span>
              {item.subtitle && <span className="adw-row-meta">{item.subtitle}</span>}
            </span>
            {item.trailing && <span className="adw-row-trailing">{item.trailing}</span>}
          </div>
        )
      })}
    </div>
  )
}

/**
 * Source-list sidebar as a navigation landmark: one tab stop for the rows, arrows move focus,
 * Enter or Space selects. Selection is a rounded `--press` fill, not an accent row.
 * Section commands (+), "more" links and placeholders are ordinary buttons; `footer` rows sit at
 * the bottom of the sidebar.
 */
export function SidebarNav({
  label,
  sections,
  footer = noItems,
  value,
  defaultValue,
  onValueChange,
}: SidebarNavProps) {
  const navRef = useRef<HTMLElement>(null)
  const flat = [
    ...sections.flatMap(section => (section.collapse?.collapsed ? [] : section.items)),
    ...footer,
  ]
  const [own, setOwn] = useState(defaultValue)
  const [focusedId, setFocusedId] = useState<string | undefined>()
  const current = value ?? own
  const tabStop =
    flat.find(item => item.id === focusedId)?.id ??
    flat.find(item => item.id === current)?.id ??
    flat[0]?.id
  const choose = (id: string) => {
    if (value === undefined) setOwn(id)
    onValueChange?.(id)
  }
  const onKeyDown = (event: KeyboardEvent) => {
    if (!(event.target instanceof HTMLElement) || !event.target.classList.contains('adw-side-item'))
      return
    const next = listStep(
      event.key,
      flat.findIndex(item => item.id === tabStop),
      flat.length
    )
    if (next !== null) {
      event.preventDefault()
      focusItem(navRef.current, '.adw-side-item', next)
    }
  }
  const row = (item: SidebarNavItem) => (
    <button
      key={item.id}
      type="button"
      className="adw-side-item"
      aria-current={item.id === current ? 'page' : undefined}
      tabIndex={item.id === tabStop ? 0 : -1}
      onFocus={() => setFocusedId(item.id)}
      onClick={() => choose(item.id)}
    >
      <span className="adw-side-icon">
        {item.icon && <Icon name={glyph(item.icon)} size={16} />}
        {item.swatch && (
          <span className="adw-dot-tag" style={{ background: swatchColor(item.swatch) }} />
        )}
        {item.status && <span className="adw-dot-tag" data-status={item.status} />}
      </span>
      <span className="adw-side-label">{item.label}</span>
      {item.count && <span className="adw-side-count">{item.count}</span>}
      {item.shortcutHint && <span className="adw-side-count">{item.shortcutHint}</span>}
    </button>
  )
  return (
    <nav ref={navRef} className="adw-sidebar-nav" aria-label={label} onKeyDown={onKeyDown}>
      {sections.map(section => {
        const collapsed = section.collapse?.collapsed === true
        return (
          <div key={section.id} className="adw-sidebar-group">
            {section.title && (
              <div
                className="adw-side-heading"
                data-has-action={section.action || section.collapse ? '' : undefined}
              >
                <span className="adw-side-heading-title">{section.title}</span>
                {section.collapse && (
                  <button
                    type="button"
                    className="adw-side-heading-action adw-side-disclosure"
                    aria-label={section.collapse.label}
                    aria-expanded={!collapsed}
                    data-collapsed={collapsed ? '' : undefined}
                    onClick={() => section.collapse?.onCollapsedChange(!collapsed)}
                  >
                    <Icon name="chevronRight" size={12} />
                  </button>
                )}
                {section.action && (
                  <WithHelpTag text={section.action.label}>
                    <button
                      type="button"
                      className="adw-side-heading-action"
                      aria-label={section.action.label}
                      onClick={section.action.onClick}
                    >
                      <Icon name={glyph(section.action.icon)} size={14} />
                    </button>
                  </WithHelpTag>
                )}
              </div>
            )}
            {collapsed ? null : section.items.length === 0 && section.placeholder ? (
              <div className="adw-side-placeholder">
                {section.placeholder.text && (
                  <span className="adw-side-placeholder-text">{section.placeholder.text}</span>
                )}
                {section.placeholder.action && (
                  <PushButton icon="plus" onClick={section.placeholder.action.onClick}>
                    {section.placeholder.action.label}
                  </PushButton>
                )}
              </div>
            ) : (
              section.items.map(row)
            )}
            {section.more && !collapsed && (
              <button type="button" className="adw-side-more" onClick={section.more.onClick}>
                {section.more.label}
              </button>
            )}
          </div>
        )
      })}
      {footer.length > 0 && <div className="adw-sidebar-footer">{footer.map(row)}</div>}
    </nav>
  )
}
