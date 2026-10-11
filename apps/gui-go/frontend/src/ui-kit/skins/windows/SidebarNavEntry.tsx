import { mergeClasses, NavItem } from '@fluentui/react-components'
import { createElement } from 'react'
import type { SidebarNavItem } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { useSidebarNavStyles } from './sidebar-nav-styles'
import { dotClass, useStatusStyles } from './status-styles'
import { swatch } from './theme'

/** One NavigationViewItem: icon, colour swatch or status dot, label, count and shortcut hint. */
export function SidebarNavEntry({ item }: { item: SidebarNavItem }) {
  const styles = useSidebarNavStyles()
  const status = useStatusStyles()
  const icon = item.status ? (
    <span className={styles.iconBox}>
      <span className={mergeClasses(status.dot, status[dotClass[item.status]])} />
    </span>
  ) : item.swatch ? (
    <span className={styles.iconBox}>
      <span className={styles.swatch} style={{ backgroundColor: swatch(item.swatch) }} />
    </span>
  ) : item.icon ? (
    createElement(iconFor(item.icon))
  ) : undefined
  return (
    <NavItem value={item.id} as="button" className={styles.item} icon={icon}>
      <span className={styles.label}>{item.label}</span>
      {item.count ? <span className={styles.count}>{item.count}</span> : null}
      {item.shortcutHint ? <span className={styles.hint}>{item.shortcutHint}</span> : null}
    </NavItem>
  )
}
