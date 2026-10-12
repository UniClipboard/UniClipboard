import {
  Button as FluentButton,
  Field,
  Input,
  makeStyles,
  tokens,
} from '@fluentui/react-components'
import { EyeOffRegular, EyeRegular } from '@fluentui/react-icons'
import { useState } from 'react'
import type { FormFieldProps } from '@/ui-kit/contract'

const useStyles = makeStyles({
  input: { width: '100%', fontSize: tokens.fontSizeBase300 },
  reveal: { minWidth: 'auto' },
})

/**
 * WinUI TextBox / PasswordBox with a header: the password reveal button sits inside the field,
 * the error replaces nothing and shows under it, the help text under that.
 */
export function FormField({
  label,
  value,
  onValueChange,
  type = 'text',
  reveal,
  placeholder,
  hint,
  error,
  disabled,
  autoFocus,
  autoComplete,
  inputRef,
}: FormFieldProps) {
  const styles = useStyles()
  const [shown, setShown] = useState(false)
  const password = type === 'password'
  return (
    <Field
      label={label}
      hint={hint}
      validationMessage={error}
      validationState={error ? 'error' : 'none'}
    >
      <Input
        ref={inputRef}
        className={styles.input}
        type={password && !shown ? 'password' : 'text'}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        // eslint-disable-next-line jsx-a11y/no-autofocus -- the caller asks for it when a step opens on this field
        autoFocus={autoFocus}
        autoComplete={autoComplete}
        spellCheck={password ? false : undefined}
        onChange={(_, data) => onValueChange(data.value)}
        onKeyDown={event => {
          // Enter that commits an input-method candidate must not submit the form.
          if (event.key === 'Enter' && (event.nativeEvent.isComposing || event.keyCode === 229))
            event.preventDefault()
        }}
        contentAfter={
          password && reveal ? (
            <FluentButton
              appearance="transparent"
              size="small"
              className={styles.reveal}
              icon={shown ? <EyeOffRegular /> : <EyeRegular />}
              aria-label={shown ? reveal.hide : reveal.show}
              aria-pressed={shown}
              disabled={disabled}
              onClick={() => setShown(current => !current)}
            />
          ) : undefined
        }
      />
    </Field>
  )
}
