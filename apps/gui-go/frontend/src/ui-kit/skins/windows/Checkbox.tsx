import { Checkbox as FluentCheckbox } from '@fluentui/react-components'
import type { CheckboxProps } from '@/ui-kit/contract'

export function Checkbox({
  checked,
  defaultChecked,
  onCheckedChange,
  label,
  labelHidden,
  disabled,
}: CheckboxProps) {
  return (
    <FluentCheckbox
      checked={checked}
      defaultChecked={defaultChecked}
      onChange={(_, data) => onCheckedChange?.(data.checked === true)}
      label={labelHidden ? undefined : label}
      aria-label={labelHidden ? label : undefined}
      disabled={disabled}
    />
  )
}
