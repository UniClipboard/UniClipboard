import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { OTPInput } from 'input-otp'
import { useId } from 'react'
import type { CodeFieldProps } from '@/ui-kit/contract'

// Code boxes drawn as WinUI TextBoxes; one text input underneath (input-otp) takes the typing,
// paste and input methods.
const useStyles = makeStyles({
  root: { display: 'flex', flexDirection: 'column', gap: '0.5rem' },
  label: { fontSize: tokens.fontSizeBase300, lineHeight: tokens.lineHeightBase300 },
  boxes: { display: 'flex', alignItems: 'center', gap: '0.5rem' },
  item: { display: 'flex', alignItems: 'center', gap: '0.5rem' },
  separator: { width: '0.625rem', height: '2px', backgroundColor: tokens.colorNeutralForeground3 },
  slot: {
    boxSizing: 'border-box',
    width: '2.5rem',
    height: '3rem',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderBottomColor: tokens.colorNeutralStrokeAccessible,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground1,
    fontSize: tokens.fontSizeBase500,
    fontWeight: tokens.fontWeightSemibold,
  },
  active: {
    borderBottomWidth: '2px',
    borderBottomColor: tokens.colorCompoundBrandStroke,
    backgroundColor: tokens.colorNeutralBackground1Pressed,
  },
  invalid: { borderBottomColor: tokens.colorPaletteRedForeground1 },
  caret: {
    width: '1px',
    height: '1.25rem',
    backgroundColor: tokens.colorNeutralForeground1,
    animationName: { '50%': { opacity: 0 } },
    animationDuration: '1s',
    animationIterationCount: 'infinite',
    animationTimingFunction: 'steps(1)',
    '@media (prefers-reduced-motion: reduce)': { animationName: 'none' },
  },
})

const digitsOnly = (text: string) => text.replace(/\D/g, '')

export function CodeField({
  label,
  length,
  value,
  onValueChange,
  invalid,
  disabled,
  autoFocus,
}: CodeFieldProps) {
  const styles = useStyles()
  const id = useId()
  const half = Math.ceil(length / 2)
  return (
    <div className={styles.root}>
      <label htmlFor={id} className={styles.label}>
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
        // eslint-disable-next-line jsx-a11y/no-autofocus -- the join step opens on the code
        autoFocus={autoFocus}
        aria-invalid={invalid || undefined}
        containerClassName={styles.boxes}
        render={({ slots }) => (
          <>
            {slots.map((slot, index) => (
              <span key={index} className={styles.item}>
                {index === half && <span className={styles.separator} aria-hidden="true" />}
                <span
                  className={mergeClasses(
                    styles.slot,
                    slot.isActive && styles.active,
                    invalid && styles.invalid
                  )}
                >
                  {slot.char}
                  {slot.hasFakeCaret && <span className={styles.caret} />}
                </span>
              </span>
            ))}
          </>
        )}
      />
    </div>
  )
}
