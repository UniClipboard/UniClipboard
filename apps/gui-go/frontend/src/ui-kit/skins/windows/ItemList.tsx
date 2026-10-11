import { List, ListItem, mergeClasses } from '@fluentui/react-components'
import { useId, useState } from 'react'
import type { KeyboardEvent } from 'react'
import type { ItemListProps } from '@/ui-kit/contract'
import { useListStyles } from './list-styles'

/**
 * WinUI ListView on Fluent List. Fluent provides the listbox roles, arrow-key movement, Space to
 * toggle and the selection checkboxes; Windows "Multiple" selection mode toggles rows on click.
 * Enter and double click activate instead of toggling, Delete reports the selection.
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
  const styles = useListStyles()
  // Fluent writes the item value as the DOM id; prefix it so the same item can appear in two lists.
  const idPrefix = useId()
  const [ownSelection, setOwnSelection] = useState(defaultSelection)
  const selected = selection ?? ownSelection
  const handleKeyDown = (event: KeyboardEvent) => {
    // Only a Delete on the row itself: Delete inside a row (a removable tag, an input) is not
    // a request to delete the selected items.
    const onRow =
      event.target instanceof HTMLElement && event.target.getAttribute('role') === 'option'
    if (
      event.key === 'Delete' &&
      onRow &&
      !event.defaultPrevented &&
      onDelete &&
      selected.length > 0
    ) {
      event.preventDefault()
      onDelete(selected)
    }
  }
  return (
    <List
      aria-label={label}
      className={styles.list}
      selectionMode={multiSelect ? 'multiselect' : 'single'}
      selectedItems={selected}
      onSelectionChange={(_, data) => {
        const ids = data.selectedItems.map(String)
        if (selection === undefined) setOwnSelection(ids)
        onSelectionChange?.(ids)
      }}
      onKeyDown={handleKeyDown}
    >
      {items.map(item => (
        <ListItem
          key={item.id}
          value={item.id}
          id={`${idPrefix}-${item.id}`}
          aria-label={item.title}
          checkmark={multiSelect ? undefined : null}
          className={mergeClasses(styles.row, multiSelect && styles.multi)}
          onAction={(_, data) => {
            const action = data.event
            if (action instanceof CustomEvent && action.detail.originalEvent.type === 'keydown') {
              action.preventDefault()
              onActivate?.(item.id)
            }
          }}
          onDoubleClick={() => onActivate?.(item.id)}
        >
          {item.leading}
          <span className={styles.text}>
            <span className={mergeClasses(styles.title, item.monospace && styles.mono)}>
              {item.title}
            </span>
            {item.subtitle ? <span className={styles.subtitle}>{item.subtitle}</span> : null}
          </span>
          {item.trailing ? <span className={styles.trailing}>{item.trailing}</span> : null}
        </ListItem>
      ))}
    </List>
  )
}
