import {
  makeStyles,
  Nav,
  NavItem,
  NavSectionHeader,
  tokens,
  useArrowNavigationGroup,
} from '@fluentui/react-components'
import { createElement, Fragment, useState } from 'react'
import type { SidebarNavProps } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { swatches } from './theme'

const useStyles = makeStyles({
  nav: { backgroundColor: 'transparent', width: '100%' },
  // Fluent Nav paints items with its drawer background; WinUI pane items are transparent at rest.
  item: {
    justifyContent: 'flex-start',
    gap: '0.75rem',
    backgroundColor: 'transparent',
    fontWeight: tokens.fontWeightRegular,
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    ':active': { backgroundColor: tokens.colorSubtleBackgroundPressed },
    '&[aria-current="page"]': {
      backgroundColor: 'var(--w11-selected)',
      fontWeight: tokens.fontWeightSemibold,
    },
  },
  label: { flexGrow: 1, textAlign: 'start' },
  count: { color: tokens.colorNeutralForeground2, fontSize: tokens.fontSizeBase200 },
  swatch: {
    display: 'inline-block',
    width: '0.75rem',
    height: '0.75rem',
    borderRadius: '0.1875rem',
  },
})

/** WinUI NavigationView pane (Left mode) on Fluent Nav: selection indicator and arrow keys. */
export function SidebarNav({
  label,
  sections,
  value,
  defaultValue,
  onValueChange,
}: SidebarNavProps) {
  const styles = useStyles()
  const [ownValue, setOwnValue] = useState(defaultValue)
  const current = value ?? ownValue
  // Fluent Nav has neither a landmark role nor arrow-key movement; the contract needs both, with
  // the pane as a single tab stop that remembers the last focused item.
  const arrowNavigation = useArrowNavigationGroup({ axis: 'vertical', memorizeCurrent: true })
  return (
    <nav aria-label={label} {...arrowNavigation}>
      <Nav
        className={styles.nav}
        selectedValue={current}
        onNavItemSelect={(_, data) => {
          if (value === undefined) setOwnValue(data.value)
          onValueChange?.(data.value)
        }}
      >
        {sections.map(section => (
          <Fragment key={section.id}>
            {section.title ? <NavSectionHeader>{section.title}</NavSectionHeader> : null}
            {section.items.map(item => (
              <NavItem
                key={item.id}
                value={item.id}
                as="button"
                className={styles.item}
                icon={
                  item.swatch ? (
                    <span
                      className={styles.swatch}
                      style={{ backgroundColor: swatches[item.swatch] }}
                    />
                  ) : item.icon ? (
                    createElement(iconFor(item.icon))
                  ) : undefined
                }
              >
                <span className={styles.label}>{item.label}</span>
                {item.count ? <span className={styles.count}>{item.count}</span> : null}
              </NavItem>
            ))}
          </Fragment>
        ))}
      </Nav>
    </nav>
  )
}
