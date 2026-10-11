import {
  Button as FluentButton,
  makeStyles,
  mergeClasses,
  tokens,
  useArrowNavigationGroup,
} from '@fluentui/react-components'
import { createElement } from 'react'
import type { SidebarNavItem, SidebarNavProps } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { WithTooltip } from './WithTooltip'

// Board WA3 / C2: the LeftCompact pane, a 48 px rail of 40 x 36 icon buttons with the selection pill; the label is
// the tooltip and the accessible name. Rows without an icon (tags, devices) are reached by expanding the pane.
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
  return (
    <nav aria-label={label} className={styles.rail} {...arrowNavigation}>
      {sections.flatMap(section => section.items.map(entry))}
      <div className={styles.footer}>{footer.map(entry)}</div>
    </nav>
  )
}
