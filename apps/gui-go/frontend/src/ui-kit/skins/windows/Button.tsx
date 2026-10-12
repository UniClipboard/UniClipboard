import { Button as FluentButton, Spinner, mergeClasses } from '@fluentui/react-components'
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
  busy,
  submitForm,
  onClick,
}: ButtonProps) {
  const elevation = useElevationStyles()
  const iconOnly = children === undefined || children === null
  const name = nameProps(iconOnly, label, tooltip)
  return (
    <WithTooltip content={name.tooltip} relationship={name.relationship}>
      <FluentButton
        appearance={appearance[variant]}
        size={fluentSize[size]}
        icon={
          busy ? <Spinner size="extra-tiny" /> : icon ? createElement(iconFor(icon)) : undefined
        }
        aria-label={name.ariaLabel}
        aria-busy={busy || undefined}
        disabled={disabled || busy}
        type={submitForm ? 'submit' : 'button'}
        form={submitForm}
        onClick={onClick ? () => onClick() : undefined}
        className={mergeClasses(
          elevation.text,
          variant === 'standard' && elevation.raised,
          variant === 'link' && elevation.link
        )}
      >
        {children}
      </FluentButton>
    </WithTooltip>
  )
}
