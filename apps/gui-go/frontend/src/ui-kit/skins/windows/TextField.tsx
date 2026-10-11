import { Field, Input } from '@fluentui/react-components'
import type { TextFieldProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract (TextBox); styling belongs to the Windows History work.
export function TextField({
  label,
  value,
  onValueChange,
  onSubmit,
  onCancel,
  placeholder,
  maxLength,
  autoFocus,
  error,
}: TextFieldProps) {
  return (
    <Field validationMessage={error} validationState={error ? 'error' : 'none'}>
      <Input
        aria-label={label}
        value={value}
        placeholder={placeholder}
        maxLength={maxLength}
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus={autoFocus}
        onChange={(_, data) => onValueChange(data.value)}
        onKeyDown={event => {
          if (event.key === 'Enter' && onSubmit) onSubmit(event.currentTarget.value)
          else if (event.key === 'Escape' && onCancel) {
            event.stopPropagation()
            onCancel()
          }
        }}
      />
    </Field>
  )
}
