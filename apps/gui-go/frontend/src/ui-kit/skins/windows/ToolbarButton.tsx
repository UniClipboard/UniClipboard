import { ToolbarButton as FluentToolbarButton } from '@fluentui/react-components'
import { createElement } from 'react'
import type { ToolbarButtonProps } from '@/ui-kit/contract'
import { nameProps } from './accessible-name'
import { useElevationStyles } from './button-appearance'
import { iconFor } from './icons'
import { WithTooltip } from './WithTooltip'

/** Command-bar item: the primary command is an accent button, the rest are subtle. */
export function ToolbarButton({
  variant = 'subtle',
  icon,
  children,
  label,
  tooltip,
  disabled,
  onClick,
}: ToolbarButtonProps) {
  const elevation = useElevationStyles()
  const iconOnly = children === undefined || children === null
  const name = nameProps(label, iconOnly ? (tooltip ?? label) : tooltip)
  return (
    <WithTooltip content={name.tooltip} relationship={name.relationship}>
      <FluentToolbarButton
        appearance={variant === 'primary' ? 'primary' : 'subtle'}
        icon={icon ? createElement(iconFor(icon)) : undefined}
        aria-label={name.ariaLabel}
        disabled={disabled}
        onClick={onClick ? () => onClick() : undefined}
        className={elevation.text}
      >
        {children}
      </FluentToolbarButton>
    </WithTooltip>
  )
}
