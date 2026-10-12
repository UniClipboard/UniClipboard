import { OTPInput } from 'input-otp'
import { useId, useState } from 'react'
import type {
  ButtonRowProps,
  ChoiceListProps,
  CodeFieldProps,
  DisclosureProps,
  EntryPageProps,
  FormFieldProps,
  ProgressBarProps,
} from '@/ui-kit/contract'
import { glyph } from './glyph'
import { Icon } from './icons'
import { isImeKey } from './ime'
import { PushButton } from './parts'
import { Spinner } from './spinner'
import './entry.css'

/**
 * Status-page layout: app icon, status line, title and description centred above the content; Back
 * at the leading and the step's actions at the trailing end of the footer, as pill buttons.
 */
export function EntryPage({
  appIcon,
  status,
  title,
  description,
  children,
  back,
  actions,
}: EntryPageProps) {
  return (
    <div className="adw-entry">
      <main className="adw-entry-scroll">
        <div className="adw-entry-column">
          <header className="adw-entry-header">
            <img src={appIcon} alt="" className="adw-entry-icon" draggable={false} />
            {status && (
              <p className="adw-entry-status" data-tone={status.tone ?? 'neutral'}>
                {status.busy ? (
                  <Spinner size={13} />
                ) : (
                  status.icon && <Icon name={glyph(status.icon)} size={14} strokeWidth={2} />
                )}
                <span>{status.text}</span>
              </p>
            )}
            <h1 className="adw-entry-title" aria-live="polite">
              {title}
            </h1>
            {description && <p className="adw-entry-description">{description}</p>}
          </header>
          {children && <div className="adw-entry-body">{children}</div>}
        </div>
      </main>
      {(back || actions) && (
        <footer className="adw-entry-footer">
          {back && (
            <PushButton
              size="large"
              icon="chevronLeft"
              disabled={back.disabled}
              onClick={() => back.onClick()}
            >
              {back.label}
            </PushButton>
          )}
          <span className="adw-entry-spacer" />
          {actions}
        </footer>
      )}
    </div>
  )
}

export function ButtonRow({ children }: ButtonRowProps) {
  return <div className="adw-button-row">{children}</div>
}

/** Entry with the label above, the help and the error under it. */
export function FormField({
  id: providedId,
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
  const generatedId = useId()
  const id = providedId ?? generatedId
  const [shown, setShown] = useState(false)
  const password = type === 'password'
  const describedBy = [error && `${id}-error`, hint && `${id}-hint`].filter(Boolean).join(' ')
  return (
    <div className="adw-form-field">
      <label htmlFor={id} className="adw-form-label">
        {label}
      </label>
      <span className="adw-form-control" data-password={password ? '' : undefined}>
        <input
          id={id}
          ref={inputRef}
          className="adw-form-input"
          type={password && !shown ? 'password' : 'text'}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          // The caller asks for focus when the step opens on this field.
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus={autoFocus}
          autoComplete={autoComplete}
          spellCheck={password ? false : undefined}
          autoCapitalize={password ? 'off' : undefined}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy || undefined}
          onChange={event => onValueChange(event.target.value)}
          onKeyDown={event => {
            // Return that commits an input-method candidate must not submit the form.
            if (event.key === 'Enter' && isImeKey(event)) event.preventDefault()
          }}
        />
        {password && reveal && (
          <button
            type="button"
            className="adw-form-reveal"
            aria-label={shown ? reveal.hide : reveal.show}
            aria-pressed={shown}
            disabled={disabled}
            onClick={() => setShown(current => !current)}
          >
            <Icon name={shown ? 'eyeOff' : 'eye'} size={14} strokeWidth={1.9} />
          </button>
        )}
      </span>
      {error && (
        <p id={`${id}-error`} role="alert" className="adw-form-error">
          <Icon name="xCircle" size={12} strokeWidth={2.2} />
          <span>{error}</span>
        </p>
      )}
      {hint && (
        <p id={`${id}-hint`} className="adw-form-hint">
          {hint}
        </p>
      )}
    </div>
  )
}

const digitsOnly = (text: string) => text.replace(/\D/g, '')

/** One text input under the boxes (input-otp), so paste, autofill and input methods behave as in a field. */
export function CodeField({
  id: providedId,
  label,
  length,
  value,
  onValueChange,
  invalid,
  disabled,
  autoFocus,
}: CodeFieldProps) {
  const generatedId = useId()
  const id = providedId ?? generatedId
  const half = Math.ceil(length / 2)
  return (
    <div className="adw-form-field adw-otp-field">
      <label htmlFor={id} className="adw-form-label">
        {label}
      </label>
      <OTPInput
        id={id}
        maxLength={length}
        inputMode="numeric"
        pattern="^[0-9]*$"
        value={value}
        onChange={next => onValueChange(digitsOnly(next).slice(0, length))}
        pasteTransformer={digitsOnly}
        disabled={disabled}
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus={autoFocus}
        aria-invalid={invalid || undefined}
        containerClassName="adw-otp"
        render={({ slots }) => (
          <>
            {slots.map((slot, index) => (
              <span key={index} className="adw-otp-group-item">
                {index === half && <span className="adw-otp-separator" aria-hidden="true" />}
                <span
                  className="adw-otp-slot"
                  data-active={slot.isActive ? '' : undefined}
                  data-invalid={invalid ? '' : undefined}
                >
                  {slot.char}
                  {slot.hasFakeCaret && <span className="adw-otp-caret" />}
                </span>
              </span>
            ))}
          </>
        )}
      />
    </div>
  )
}

/** Progress bar: determinate with a value, a moving segment without. */
export function ProgressBar({ label, value }: ProgressBarProps) {
  const determinate = value !== undefined
  return (
    <div
      className="adw-progress"
      role="progressbar"
      aria-label={label}
      aria-valuemin={determinate ? 0 : undefined}
      aria-valuemax={determinate ? 100 : undefined}
      aria-valuenow={determinate ? Math.round(value) : undefined}
      data-indeterminate={determinate ? undefined : ''}
    >
      <span style={determinate ? { width: `${Math.min(100, Math.max(0, value))}%` } : undefined} />
    </div>
  )
}

/** Boxed list of navigation rows: coloured symbol tile, title, description, chevron. */
export function ChoiceList({ label, title, items, onSelect, disabled }: ChoiceListProps) {
  const headingId = useId()
  return (
    <section
      className="adw-choices"
      aria-label={title ? undefined : label}
      aria-labelledby={title ? headingId : undefined}
    >
      {title && (
        <h2 id={headingId} className="adw-choices-title">
          {title}
        </h2>
      )}
      <div className="adw-choices-group">
        {items.map(item => (
          <button
            key={item.id}
            data-testid={item['data-testid']}
            type="button"
            className="adw-choice"
            data-symbol={item.icon}
            disabled={disabled}
            onClick={() => onSelect(item.id)}
          >
            {item.icon && (
              <span className="adw-choice-tile">
                <Icon name={glyph(item.icon)} size={16} strokeWidth={2} />
              </span>
            )}
            <span className="adw-choice-text">
              <span className="adw-choice-title">{item.title}</span>
              {item.description && (
                <span className="adw-choice-description">{item.description}</span>
              )}
            </span>
            <Icon name="chevronRight" size={13} strokeWidth={2.2} />
          </button>
        ))}
      </div>
    </section>
  )
}

/** Disclosure triangle with its label; the content opens below. */
export function Disclosure({ label, defaultOpen, children }: DisclosureProps) {
  return (
    <details className="adw-disclosure" open={defaultOpen}>
      <summary>
        <Icon name="chevronRight" size={11} strokeWidth={2.6} />
        {label}
      </summary>
      <div className="adw-disclosure-content">{children}</div>
    </details>
  )
}
