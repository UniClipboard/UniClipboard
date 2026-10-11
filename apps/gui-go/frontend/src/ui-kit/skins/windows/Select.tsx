import { Dropdown, makeStyles, mergeClasses, Option, tokens } from '@fluentui/react-components'
import { useState } from 'react'
import type { SelectProps } from '@/ui-kit/contract'

// A filter is a compact filled drop-down sized to its text; the filter's name (placeholder) reads
// as normal text, and a chosen value turns it into the accent-tinted "on" state.
const useFilterStyles = makeStyles({
  root: { minWidth: 'unset', borderRadius: tokens.borderRadiusMedium },
  button: { color: tokens.colorNeutralForeground1, paddingInlineEnd: '0.25rem' },
  active: {
    backgroundColor: tokens.colorBrandBackground2,
    ':hover': { backgroundColor: tokens.colorBrandBackground2Hover },
  },
  activeButton: { color: tokens.colorBrandForeground2 },
})

/** WinUI ComboBox (non-editable): Fluent Dropdown with listbox keyboard handling and type-ahead. */
export function Select({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
  placeholder,
  disabled,
  purpose,
}: SelectProps) {
  const filterStyles = useFilterStyles()
  const [ownValue, setOwnValue] = useState(defaultValue)
  const current = value ?? ownValue
  // A value that is not one of the items is "nothing chosen": the placeholder shows.
  const selected = items.find(item => item.value === current)
  const filter = purpose === 'filter'
  return (
    <Dropdown
      aria-label={label}
      placeholder={placeholder}
      disabled={disabled}
      value={selected?.label ?? ''}
      selectedOptions={selected ? [selected.value] : []}
      onOptionSelect={(_, data) => {
        if (data.optionValue === undefined) return
        if (value === undefined) setOwnValue(data.optionValue)
        onValueChange?.(data.optionValue)
      }}
      {...(filter && {
        appearance: 'filled-darker' as const,
        size: 'small' as const,
        className: mergeClasses(filterStyles.root, selected && filterStyles.active),
        button: {
          className: mergeClasses(filterStyles.button, selected && filterStyles.activeButton),
        },
      })}
      data-active={filter && selected ? '' : undefined}
    >
      {items.map(item => (
        <Option key={item.value} value={item.value}>
          {item.label}
        </Option>
      ))}
    </Dropdown>
  )
}
