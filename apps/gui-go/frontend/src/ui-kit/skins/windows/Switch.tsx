import { Switch as FluentSwitch } from '@fluentui/react-components'
import type { SwitchProps } from '@/ui-kit/contract'

export function Switch({
  checked,
  defaultChecked,
  onCheckedChange,
  label,
  labelHidden,
  disabled,
}: SwitchProps) {
  return (
    <FluentSwitch
      checked={checked}
      defaultChecked={defaultChecked}
      onChange={(_, data) => onCheckedChange?.(data.checked)}
      label={labelHidden ? undefined : label}
      aria-label={labelHidden ? label : undefined}
      disabled={disabled}
    />
  )
}
