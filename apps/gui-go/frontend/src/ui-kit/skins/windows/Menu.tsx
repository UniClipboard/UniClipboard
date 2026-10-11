import {
  Button as FluentButton,
  CounterBadge,
  Menu as FluentMenu,
  MenuButton,
  MenuPopover,
  MenuTrigger,
  mergeClasses,
  Tooltip,
} from '@fluentui/react-components'
import { createElement } from 'react'
import type { MenuProps } from '@/ui-kit/contract'
import { appearance, useElevationStyles } from './button-appearance'
import { iconFor } from './icons'
import { useMenuStyles } from './menu-styles'
import { MenuEntries } from './MenuEntries'

/** Icon-only triggers are command-bar "more" buttons; labelled ones are WinUI DropDownButtons. */
export function Menu({
  label,
  icon,
  iconOnly,
  variant = 'standard',
  count,
  items,
  onAction,
  open,
  onOpenChange,
}: MenuProps) {
  const styles = useMenuStyles()
  const elevation = useElevationStyles()
  const iconElement = icon ? createElement(iconFor(icon)) : undefined
  const trigger = iconOnly ? (
    // Fluent Tooltip, not WithTooltip: MenuTrigger's props must pass through to the button.
    <Tooltip content={label} relationship="inaccessible" withArrow={false}>
      <FluentButton appearance={appearance[variant]} icon={iconElement} aria-label={label} />
    </Tooltip>
  ) : (
    <MenuButton
      appearance={appearance[variant]}
      icon={iconElement}
      size="small"
      className={mergeClasses(elevation.text, variant === 'standard' && elevation.raised)}
    >
      {label}
      {count ? (
        <CounterBadge count={count} size="small" style={{ marginInlineStart: '0.375rem' }} />
      ) : null}
    </MenuButton>
  )
  return (
    <FluentMenu open={open} onOpenChange={(_, data) => onOpenChange?.(data.open)}>
      <MenuTrigger>{trigger}</MenuTrigger>
      <MenuPopover className={styles.popover}>
        <MenuEntries items={items} onAction={onAction} />
      </MenuPopover>
    </FluentMenu>
  )
}
