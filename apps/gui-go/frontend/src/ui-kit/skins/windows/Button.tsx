import { Button as FluentButton, mergeClasses } from '@fluentui/react-components'
import { createElement } from 'react'
import type { ButtonProps } from '@/ui-kit/contract'
import { nameProps } from './accessible-name'
import { appearance, fluentSize, useElevationStyles } from './button-appearance'
import { iconFor } from './icons'
import { WithTooltip } from './WithTooltip'

export function Button({
  variant = 'standard',
  size = 'medium',
  icon,
  children,
  label,
  tooltip,
  disabled,
  onClick,
}: ButtonProps) {
  const elevation = useElevationStyles()
  const iconOnly = children === undefined || children === null
  const name = nameProps(label, iconOnly ? (tooltip ?? label) : tooltip)
  return (
    <WithTooltip content={name.tooltip} relationship={name.relationship}>
      <FluentButton
        appearance={appearance[variant]}
        size={fluentSize[size]}
        icon={icon ? createElement(iconFor(icon)) : undefined}
        aria-label={name.ariaLabel}
        disabled={disabled}
        onClick={onClick ? () => onClick() : undefined}
        className={mergeClasses(elevation.text, variant === 'standard' && elevation.raised)}
      >
        {children}
      </FluentButton>
    </WithTooltip>
  )
}
