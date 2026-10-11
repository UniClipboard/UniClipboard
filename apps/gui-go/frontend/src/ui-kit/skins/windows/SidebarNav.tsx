import {
  Button as FluentButton,
  Nav,
  NavSectionHeader,
  useArrowNavigationGroup,
} from '@fluentui/react-components'
import { Fragment, useState } from 'react'
import type { SidebarNavItem, SidebarNavProps } from '@/ui-kit/contract'
import { Button } from './Button'
import { useSidebarDisplay } from './sidebar-display'
import { useSidebarNavStyles } from './sidebar-nav-styles'
import { SidebarNavCompact } from './SidebarNavCompact'
import { SidebarNavEntry } from './SidebarNavEntry'

// A shared empty default, so an omitted prop does not create a new array on every render.
const noItems: SidebarNavItem[] = []

/**
 * WinUI NavigationView pane (Left mode) on Fluent Nav: selection indicator and arrow keys.
 * As in NavigationView, the whole pane is one arrow-key group with a single tab stop: section
 * commands, "more" rows, placeholder actions and the footer items are reached with the arrows too.
 */
export function SidebarNav({
  label,
  sections,
  footer = noItems,
  value,
  defaultValue,
  onValueChange,
}: SidebarNavProps) {
  const styles = useSidebarNavStyles()
  const [ownValue, setOwnValue] = useState(defaultValue)
  const current = value ?? ownValue
  // Fluent Nav has neither a landmark role nor arrow-key movement; the contract needs both, with
  // the pane as a single tab stop that remembers the last focused item.
  const arrowNavigation = useArrowNavigationGroup({ axis: 'vertical', memorizeCurrent: true })
  const display = useSidebarDisplay()
  const select = (id: string) => {
    if (value === undefined) setOwnValue(id)
    onValueChange?.(id)
  }
  if (display === 'compact') {
    return (
      <SidebarNavCompact
        label={label}
        sections={sections}
        footer={footer}
        current={current}
        select={select}
      />
    )
  }
  return (
    <nav aria-label={label} className={styles.root} {...arrowNavigation}>
      <Nav
        className={styles.nav}
        // Always controlled: Fluent fixes controlled vs uncontrolled on its first render.
        selectedValue={current ?? ''}
        onNavItemSelect={(_, data) => select(data.value)}
      >
        {sections.map(section => (
          <Fragment key={section.id}>
            {section.title ? (
              <div className={styles.heading}>
                <NavSectionHeader className={styles.headingTitle}>{section.title}</NavSectionHeader>
                {section.action ? (
                  <Button
                    variant="subtle"
                    size="small"
                    icon={section.action.icon}
                    label={section.action.label}
                    onClick={section.action.onClick}
                  />
                ) : null}
              </div>
            ) : null}
            {section.items.length === 0 && section.placeholder ? (
              <div className={styles.placeholder}>
                {section.placeholder.text ? (
                  <span className={styles.placeholderText}>{section.placeholder.text}</span>
                ) : null}
                {section.placeholder.action ? (
                  <Button size="small" icon="add" onClick={section.placeholder.action.onClick}>
                    {section.placeholder.action.label}
                  </Button>
                ) : null}
              </div>
            ) : (
              section.items.map(item => <SidebarNavEntry key={item.id} item={item} />)
            )}
            {section.more ? (
              <FluentButton
                appearance="transparent"
                className={styles.more}
                onClick={() => section.more?.onClick()}
              >
                {section.more.label}
              </FluentButton>
            ) : null}
          </Fragment>
        ))}
        {footer.length > 0 ? (
          <div className={styles.footer}>
            {footer.map(item => (
              <SidebarNavEntry key={item.id} item={item} />
            ))}
          </div>
        ) : null}
      </Nav>
    </nav>
  )
}
