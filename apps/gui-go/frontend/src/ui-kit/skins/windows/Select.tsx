import { Dropdown, Option } from '@fluentui/react-components'
import { useState } from 'react'
import type { SelectProps } from '@/ui-kit/contract'

/** WinUI ComboBox (non-editable): Fluent Dropdown with listbox keyboard handling and type-ahead. */
export function Select({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
  placeholder,
  disabled,
}: SelectProps) {
  const [ownValue, setOwnValue] = useState(defaultValue)
  const current = value ?? ownValue
  const selected = items.find(item => item.value === current)
  return (
    <Dropdown
      aria-label={label}
      placeholder={placeholder}
      disabled={disabled}
      value={selected?.label ?? ''}
      selectedOptions={current === undefined ? [] : [current]}
      onOptionSelect={(_, data) => {
        if (data.optionValue === undefined) return
        if (value === undefined) setOwnValue(data.optionValue)
        onValueChange?.(data.optionValue)
      }}
    >
      {items.map(item => (
        <Option key={item.value} value={item.value}>
          {item.label}
        </Option>
      ))}
    </Dropdown>
  )
}
