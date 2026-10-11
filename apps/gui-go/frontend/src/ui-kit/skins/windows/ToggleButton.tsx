import { mergeClasses, ToggleButton as FluentToggleButton } from '@fluentui/react-components'
import { createElement, useState } from 'react'
import type { ToggleButtonProps } from '@/ui-kit/contract'
import { nameProps } from './accessible-name'
import { fluentSize, useElevationStyles } from './button-appearance'
import { iconFor } from './icons'
import { WithTooltip } from './WithTooltip'

/** WinUI ToggleButton: checked = accent fill and the filled icon. */
export function ToggleButton({
  pressed,
  defaultPressed = false,
  onPressedChange,
  size = 'medium',
  icon,
  children,
  label,
  tooltip,
  disabled,
}: ToggleButtonProps) {
  const elevation = useElevationStyles()
  const [ownPressed, setOwnPressed] = useState(defaultPressed)
  const checked = pressed ?? ownPressed
  const iconOnly = children === undefined || children === null
  const name = nameProps(iconOnly, label, tooltip)
  const toggle = () => {
    if (pressed === undefined) setOwnPressed(!checked)
    onPressedChange?.(!checked)
  }
  return (
    <WithTooltip content={name.tooltip} relationship={name.relationship}>
      <FluentToggleButton
        checked={checked}
        appearance={checked ? 'primary' : 'secondary'}
        size={fluentSize[size]}
        icon={icon ? createElement(iconFor(icon, checked)) : undefined}
        aria-label={name.ariaLabel}
        disabled={disabled}
        onClick={toggle}
        className={mergeClasses(elevation.text, !checked && elevation.raised)}
      >
        {children}
      </FluentToggleButton>
    </WithTooltip>
  )
}
