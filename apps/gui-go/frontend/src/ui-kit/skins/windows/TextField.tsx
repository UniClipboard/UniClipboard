import { Field, Input, makeStyles, tokens } from '@fluentui/react-components'
import type { TextFieldProps } from '@/ui-kit/contract'

const useStyles = makeStyles({ input: { minWidth: '12rem', fontSize: tokens.fontSizeBase300 } })

/** WinUI TextBox (Fluent Input: control fill, accent underline on focus); the error shows below it. */
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
  const styles = useStyles()
  return (
    <Field validationMessage={error} validationState={error ? 'error' : 'none'}>
      <Input
        className={styles.input}
        aria-label={label}
        value={value}
        placeholder={placeholder}
        maxLength={maxLength}
        // eslint-disable-next-line jsx-a11y/no-autofocus -- the caller asks for it, e.g. renaming in place
        autoFocus={autoFocus}
        onChange={(_, data) => onValueChange(data.value)}
        onKeyDown={event => {
          if (event.key === 'Enter' && onSubmit) {
            event.preventDefault()
            onSubmit(event.currentTarget.value)
          } else if (event.key === 'Escape' && onCancel) {
            // Escape cancels the edit, not the dialog around it.
            event.stopPropagation()
            onCancel()
          }
        }}
      />
    </Field>
  )
}
