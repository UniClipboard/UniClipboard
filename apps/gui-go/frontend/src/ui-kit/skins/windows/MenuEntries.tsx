import {
  Menu,
  MenuDivider,
  MenuItem,
  MenuList,
  MenuPopover,
  MenuTrigger,
  mergeClasses,
} from '@fluentui/react-components'
import { CheckmarkRegular } from '@fluentui/react-icons'
import { createElement } from 'react'
import type { MenuEntry } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { useMenuStyles } from './menu-styles'
import { swatch } from './theme'

interface MenuEntriesProps {
  items: MenuEntry[]
  onAction: (id: string) => void
}

/**
 * WinUI MenuFlyout items: icon or colour swatch, label, shortcut or count; a checked item carries the
 * checkmark at its end (the current choice, e.g. a tag's colour); an item with children opens a
 * cascading submenu (MenuFlyoutSubItem), whose items report through the same `onAction`.
 */
export function MenuEntries({ items, onAction }: MenuEntriesProps) {
  const styles = useMenuStyles()
  return (
    <MenuList>
      {items.map(item => {
        if ('kind' in item) return <MenuDivider key={item.id} />
        const icon = item.swatch ? (
          <span className={styles.swatch} style={{ backgroundColor: swatch(item.swatch) }} />
        ) : item.icon ? (
          createElement(iconFor(item.icon))
        ) : undefined
        if (item.children) {
          return (
            <Menu key={item.id}>
              <MenuTrigger disableButtonEnhancement>
                <MenuItem icon={icon} disabled={item.disabled} className={styles.item}>
                  {item.label}
                </MenuItem>
              </MenuTrigger>
              <MenuPopover className={styles.popover}>
                <MenuEntries items={item.children} onAction={onAction} />
              </MenuPopover>
            </Menu>
          )
        }
        const secondary = item.checked ? (
          <CheckmarkRegular aria-hidden="true" />
        ) : (
          (item.shortcut ?? (item.count === undefined ? undefined : String(item.count)))
        )
        return (
          <MenuItem
            key={item.id}
            icon={icon}
            secondaryContent={secondary}
            disabled={item.disabled}
            // The checkmark is drawn; assistive tech reads the state.
            aria-checked={item.checked === undefined ? undefined : item.checked}
            role={item.checked === undefined ? undefined : 'menuitemradio'}
            className={mergeClasses(styles.item, item.destructive && styles.destructive)}
            onClick={() => onAction(item.id)}
          >
            {item.label}
          </MenuItem>
        )
      })}
    </MenuList>
  )
}
