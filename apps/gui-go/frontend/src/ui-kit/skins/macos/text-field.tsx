import type { TextFieldProps } from '@/ui-kit/contract'
import { isImeKey } from './ime'

/** NSTextField: a square-cornered field with the system focus ring. */
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
    <span className="mac-text-field">
      <input
        className="mac-field"
        type="text"
        aria-label={label}
        aria-invalid={error ? true : undefined}
        // The field appears on request (rename, new) and takes focus at once.
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus={autoFocus}
        placeholder={placeholder}
        maxLength={maxLength}
        value={value}
        onChange={event => onValueChange(event.target.value)}
        onKeyDown={event => {
          // Escape and Enter of an open input-method composition belong to the input method.
          if (isImeKey(event)) return
          if (event.key === 'Enter' && onSubmit) {
            event.preventDefault()
            onSubmit(event.currentTarget.value)
          } else if (event.key === 'Escape' && onCancel) {
            // Escape cancels the edit, not the sheet around it.
            event.preventDefault()
            event.stopPropagation()
            onCancel()
          }
        }}
      />
      {error && (
        <span className="mac-field-error" role="alert">
          {error}
        </span>
      )}
    </span>
  )
}
