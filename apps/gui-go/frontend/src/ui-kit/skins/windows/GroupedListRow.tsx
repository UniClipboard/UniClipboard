import { mergeClasses } from '@fluentui/react-components'
import { CheckmarkFilled } from '@fluentui/react-icons'
import type { MouseEvent } from 'react'
import type { ItemListItem } from '@/ui-kit/contract'
import { useGroupedListStyles } from './grouped-list-styles'
import { useListStyles } from './list-styles'

interface GroupedListRowProps {
  item: ItemListItem
  domId: string
  selected: boolean
  /** Several rows are selected: every row shows its check box (board C3). */
  multi: boolean
  cursor: boolean
  onMouseDown: (event: MouseEvent) => void
  onContextMenu: () => void
  onDoubleClick: () => void
}

/**
 * A WinUI ListViewItem: subtle fill and accent pill when selected; while several rows are selected,
 * a check box leads every row instead of the pill (board C3). The box is visual only: the row's
 * `aria-selected` already carries the state.
 */
export function GroupedListRow({
  item,
  domId,
  selected,
  multi,
  cursor,
  onMouseDown,
  onContextMenu,
  onDoubleClick,
}: GroupedListRowProps) {
  const list = useListStyles()
  const styles = useGroupedListStyles()
  return (
    <div className={styles.cell}>
      {/* Keyboard handling lives on the listbox (aria-activedescendant); rows take pointer input. */}
      <div
        id={domId}
        className={mergeClasses(list.row, styles.row, multi && list.multi)}
        role="option"
        aria-selected={selected}
        aria-label={item.title}
        data-item-id={item.id}
        data-cursor={cursor ? '' : undefined}
        onMouseDown={onMouseDown}
        onContextMenu={onContextMenu}
        onDoubleClick={onDoubleClick}
      >
        {multi ? (
          <span
            className={mergeClasses(styles.check, selected && styles.checked)}
            aria-hidden="true"
            data-row-check={selected ? 'checked' : ''}
          >
            {selected ? <CheckmarkFilled /> : null}
          </span>
        ) : null}
        {item.leading}
        <span className={list.text}>
          <span className={mergeClasses(list.title, item.monospace && list.mono)}>
            {item.title}
          </span>
          {item.subtitle ? <span className={list.subtitle}>{item.subtitle}</span> : null}
        </span>
        {item.trailing ? <span className={list.trailing}>{item.trailing}</span> : null}
      </div>
    </div>
  )
}
