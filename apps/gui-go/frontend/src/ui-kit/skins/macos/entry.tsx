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
 * Setup Assistant layout (design page "Setup · Startup · Unlock"): app icon, status line, title
 * and description centred above the content; Back at the leading and the step's actions at the
 * trailing end of the footer, as large capsules.
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
    <div className="mac-entry">
      <main className="mac-entry-scroll">
        <div className="mac-entry-column">
          <header className="mac-entry-header">
            <img src={appIcon} alt="" className="mac-entry-icon" draggable={false} />
            {status && (
              <p className="mac-entry-status" data-tone={status.tone ?? 'neutral'}>
                {status.busy ? (
                  <Spinner size={13} />
                ) : (
                  status.icon && <Icon name={glyph(status.icon)} size={13} strokeWidth={2} />
                )}
                <span>{status.text}</span>
              </p>
            )}
            <h1 className="mac-entry-title" aria-live="polite">
              {title}
            </h1>
            {description && <p className="mac-entry-description">{description}</p>}
          </header>
          {children && <div className="mac-entry-body">{children}</div>}
        </div>
      </main>
      {(back || actions) && (
        <footer className="mac-entry-footer">
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
          <span className="mac-entry-spacer" />
          {actions}
        </footer>
      )}
    </div>
  )
}

export function ButtonRow({ children }: ButtonRowProps) {
  return <div className="mac-button-row">{children}</div>
}

/** NSTextField / NSSecureTextField with the label above, the help and the error under it. */
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
    <div className="mac-form-field">
      <label htmlFor={id} className="mac-form-label">
        {label}
      </label>
      <span className="mac-form-control" data-password={password ? '' : undefined}>
        <input
          id={id}
          ref={inputRef}
          className="mac-form-input"
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
            className="mac-form-reveal"
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
        <p id={`${id}-error`} role="alert" className="mac-form-error">
          <Icon name="xCircle" size={12} strokeWidth={2.2} />
          <span>{error}</span>
        </p>
      )}
      {hint && (
        <p id={`${id}-hint`} className="mac-form-hint">
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
    <div className="mac-form-field mac-otp-field">
      <label htmlFor={id} className="mac-form-label">
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
        containerClassName="mac-otp"
        render={({ slots }) => (
          <>
            {slots.map((slot, index) => (
              <span key={index} className="mac-otp-group-item">
                {index === half && <span className="mac-otp-separator" aria-hidden="true" />}
                <span
                  className="mac-otp-slot"
                  data-active={slot.isActive ? '' : undefined}
                  data-invalid={invalid ? '' : undefined}
                >
                  {slot.char}
                  {slot.hasFakeCaret && <span className="mac-otp-caret" />}
                </span>
              </span>
            ))}
          </>
        )}
      />
    </div>
  )
}

/** NSProgressIndicator bar: determinate with a value, a moving segment without. */
export function ProgressBar({ label, value }: ProgressBarProps) {
  const determinate = value !== undefined
  return (
    <div
      className="mac-progress"
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

/** Grouped rows (System Settings): coloured symbol tile, title, description, chevron. */
export function ChoiceList({ label, title, items, onSelect, disabled }: ChoiceListProps) {
  const headingId = useId()
  return (
    <section
      className="mac-choices"
      aria-label={title ? undefined : label}
      aria-labelledby={title ? headingId : undefined}
    >
      {title && (
        <h2 id={headingId} className="mac-choices-title">
          {title}
        </h2>
      )}
      <div className="mac-choices-group">
        {items.map(item => (
          <button
            key={item.id}
            data-testid={item['data-testid']}
            type="button"
            className="mac-choice"
            data-symbol={item.icon}
            disabled={disabled}
            onClick={() => onSelect(item.id)}
          >
            {item.icon && (
              <span className="mac-choice-tile">
                <Icon name={glyph(item.icon)} size={16} strokeWidth={2} />
              </span>
            )}
            <span className="mac-choice-text">
              <span className="mac-choice-title">{item.title}</span>
              {item.description && (
                <span className="mac-choice-description">{item.description}</span>
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
    <details className="mac-disclosure" open={defaultOpen}>
      <summary>
        <Icon name="chevronRight" size={11} strokeWidth={2.6} />
        {label}
      </summary>
      <div className="mac-disclosure-content">{children}</div>
    </details>
  )
}
