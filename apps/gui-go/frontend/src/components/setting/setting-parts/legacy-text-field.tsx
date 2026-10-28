import { Input } from '@/components/ui'
import type { TextFieldProps } from '@/ui-kit/contract'

export function SettingTextField({
  label,
  value,
  onValueChange,
  placeholder,
  error,
}: Pick<TextFieldProps, 'label' | 'value' | 'onValueChange' | 'placeholder' | 'error'>) {
  return (
    <div className="flex flex-col gap-1">
      <Input
        aria-label={label}
        aria-invalid={error ? true : undefined}
        value={value}
        placeholder={placeholder}
        onChange={event => onValueChange(event.target.value)}
      />
      {error && (
        <span className="text-ui-body text-destructive" role="alert">
          {error}
        </span>
      )}
    </div>
  )
}
