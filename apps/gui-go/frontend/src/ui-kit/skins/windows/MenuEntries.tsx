import { MenuDivider, MenuItem, MenuList, mergeClasses } from '@fluentui/react-components'
import { createElement } from 'react'
import type { MenuEntry } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { useMenuStyles } from './menu-styles'
import { swatch } from './theme'

interface MenuEntriesProps {
  items: MenuEntry[]
  onAction: (id: string) => void
}

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
        const secondary =
          item.shortcut ?? (item.count === undefined ? undefined : String(item.count))
        return (
          <MenuItem
            key={item.id}
            icon={icon}
            secondaryContent={secondary}
            disabled={item.disabled}
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
