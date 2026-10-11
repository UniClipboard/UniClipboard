import {
  Menu,
  MenuDivider,
  MenuGroupHeader,
  MenuItem,
  MenuList,
  MenuPopover,
  MenuTrigger,
  Button as FluentButton,
  makeStyles,
  mergeClasses,
  tokens,
  useArrowNavigationGroup,
} from '@fluentui/react-components'
import { createElement } from 'react'
import type { SidebarNavItem, SidebarNavProps, SidebarNavSection } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { useMenuStyles } from './menu-styles'
import { swatch } from './theme'
import { WithTooltip } from './WithTooltip'

// Board WA3 / C2: the LeftCompact pane, a 48 px rail of 40 x 36 icon buttons with the selection pill; the label is
// the tooltip and the accessible name. A section with an icon (tags, devices) is one rail button that opens
// its rows in a flyout beside the rail, as in WA3.
const useStyles = makeStyles({
  rail: {
    flexGrow: 1,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: '0.125rem',
    paddingBlock: '0.25rem',
  },
  item: {
    position: 'relative',
    width: '2.5rem',
    minWidth: '2.5rem',
    height: '2.25rem',
    padding: 0,
    color: tokens.colorNeutralForeground1,
    '&[aria-current="page"]': { backgroundColor: 'var(--w11-selected)' },
    '&[aria-current="page"] svg': { color: tokens.colorBrandForeground1 },
    '&[aria-current="page"]::before': {
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
  footer: { marginTop: 'auto', display: 'flex', flexDirection: 'column', alignItems: 'center' },
  divider: {
    width: '1.5rem',
    height: '1px',
    marginBlock: '0.375rem',
    backgroundColor: 'var(--w11-divider)',
  },
  swatch: {
    display: 'inline-block',
    width: '0.75rem',
    height: '0.75rem',
    borderRadius: '0.1875rem',
  },
  current: { fontWeight: tokens.fontWeightSemibold },
})

export function SidebarNavCompact({
  label,
  sections,
  footer,
  current,
  select,
}: Pick<SidebarNavProps, 'label' | 'sections'> & {
  footer: SidebarNavItem[]
  current: string | undefined
  select: (id: string) => void
}) {
  const styles = useStyles()
  const menuStyles = useMenuStyles()
  const arrowNavigation = useArrowNavigationGroup({ axis: 'vertical', memorizeCurrent: true })
  const entry = (item: SidebarNavItem) =>
    item.icon ? (
      <WithTooltip key={item.id} content={item.label} relationship="inaccessible">
        <FluentButton
          aria-label={item.label}
          appearance="subtle"
          className={mergeClasses(styles.item)}
          icon={createElement(iconFor(item.icon))}
          aria-current={item.id === current ? 'page' : undefined}
          onClick={() => select(item.id)}
        />
      </WithTooltip>
    ) : null
  const flyout = (section: SidebarNavSection) => {
    const icon = section.icon
    if (!icon) return null
    const active = section.items.some(item => item.id === current)
    return (
      <Menu key={section.id} positioning={{ position: 'after', align: 'top' }}>
        <MenuTrigger disableButtonEnhancement>
          <WithTooltip content={section.title ?? section.id} relationship="inaccessible">
            <FluentButton
              aria-label={section.title ?? section.id}
              appearance="subtle"
              className={styles.item}
              icon={createElement(iconFor(icon))}
              aria-current={active ? 'page' : undefined}
            />
          </WithTooltip>
        </MenuTrigger>
        <MenuPopover className={menuStyles.popover}>
          <MenuList>
            {section.title ? <MenuGroupHeader>{section.title}</MenuGroupHeader> : null}
            {section.items.map(item => (
              <MenuItem
                key={item.id}
                className={mergeClasses(menuStyles.item, item.id === current && styles.current)}
                icon={
                  item.swatch ? (
                    <span
                      className={styles.swatch}
                      style={{ backgroundColor: swatch(item.swatch) }}
                    />
                  ) : item.icon ? (
                    createElement(iconFor(item.icon))
                  ) : undefined
                }
                secondaryContent={item.count}
                onClick={() => select(item.id)}
              >
                {item.label}
              </MenuItem>
            ))}
            {section.items.length === 0 && section.placeholder?.action ? (
              <MenuItem onClick={() => section.placeholder?.action?.onClick()}>
                {section.placeholder.action.label}
              </MenuItem>
            ) : null}
            {section.more ? (
              <>
                <MenuDivider />
                <MenuItem onClick={() => section.more?.onClick()}>{section.more.label}</MenuItem>
              </>
            ) : null}
          </MenuList>
        </MenuPopover>
      </Menu>
    )
  }
  const sectioned = sections.filter(section => section.icon)
  return (
    <nav aria-label={label} className={styles.rail} {...arrowNavigation}>
      {sections.filter(section => !section.icon).flatMap(section => section.items.map(entry))}
      {sectioned.length > 0 ? <span className={styles.divider} aria-hidden="true" /> : null}
      {sectioned.map(flyout)}
      <div className={styles.footer}>{footer.map(entry)}</div>
    </nav>
  )
}
