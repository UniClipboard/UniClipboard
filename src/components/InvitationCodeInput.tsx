import { type ComponentProps } from 'react'
import { InputOTP, InputOTPGroup, InputOTPSlot } from '@/components/ui/input-otp'
import { cn } from '@/lib/utils'

export const INVITATION_CODE_LENGTH = 8

const ALLOWED_CHARS = /[A-Z0-9]/

type Props = Omit<
  ComponentProps<typeof InputOTP>,
  'maxLength' | 'pattern' | 'value' | 'onChange' | 'render' | 'children'
> & {
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  invalid?: boolean
  className?: string
}

/**
 * 8-slot OTP-style invitation code input, restyled to read as a single
 * underlined field. Internally still backed by `input-otp`, so caret
 * tracking, paste-splitting, and arrow/backspace navigation come for free.
 *
 * Visual: each slot drops the boxed look and keeps only a bottom rule;
 * slots within a group are flush, and a small gap between the two groups
 * stands in for the `XXXX-XXXX` separator.
 */
const slotClass = cn(
  // sizing & typography
  'size-11 sm:size-12 text-xl sm:text-2xl font-mono font-medium uppercase',
  // strip the boxed default — keep only a bottom rule
  'rounded-none first:rounded-none last:rounded-none',
  'border-0 first:border-l-0 border-b border-border/60 bg-transparent',
  // active slot: highlight underline only, suppress the default ring
  'data-[active=true]:border-b-primary data-[active=true]:ring-0',
  // smooth color transitions on focus changes
  'transition-colors'
)

const invalidSlotClass =
  'border-b-destructive text-destructive data-[active=true]:border-b-destructive'

export function InvitationCodeInput({
  value,
  onChange,
  disabled,
  invalid,
  className,
  ...rest
}: Props) {
  const handleChange = (next: string) => {
    const cleaned = next
      .toUpperCase()
      .split('')
      .filter(c => ALLOWED_CHARS.test(c))
      .join('')
      .slice(0, INVITATION_CODE_LENGTH)
    onChange(cleaned)
  }

  // Strip the `XXXX-XXXX` hyphen on paste so the underlying `<input>`'s
  // maxLength=8 does not lop off the final character of a 9-char clipboard
  // payload before our onChange filter runs. Passing this transformer also
  // forces input-otp to route paste through JS (preventDefault + manual
  // setValue) on non-iOS browsers, sidestepping the maxLength truncation.
  const handlePaste = (text: string) =>
    text
      .toUpperCase()
      .split('')
      .filter(c => ALLOWED_CHARS.test(c))
      .join('')

  const finalSlotClass = cn(slotClass, invalid && invalidSlotClass)

  return (
    <InputOTP
      maxLength={INVITATION_CODE_LENGTH}
      value={value}
      onChange={handleChange}
      pasteTransformer={handlePaste}
      disabled={disabled}
      aria-invalid={invalid || undefined}
      containerClassName={cn('justify-center gap-3', className)}
      {...rest}
    >
      <InputOTPGroup className="gap-0 has-aria-invalid:ring-0">
        {[0, 1, 2, 3].map(i => (
          <InputOTPSlot key={i} index={i} className={finalSlotClass} />
        ))}
      </InputOTPGroup>
      <InputOTPGroup className="gap-0 has-aria-invalid:ring-0">
        {[4, 5, 6, 7].map(i => (
          <InputOTPSlot key={i} index={i} className={finalSlotClass} />
        ))}
      </InputOTPGroup>
    </InputOTP>
  )
}

/** Format a raw 8-char code as `XXXX-XXXX` for display surfaces. */
export function formatInvitationCode(raw: string): string {
  const clean = raw.toUpperCase().replace(/[^A-Z0-9]/g, '')
  if (clean.length <= 4) return clean
  return `${clean.slice(0, 4)}-${clean.slice(4, INVITATION_CODE_LENGTH)}`
}
