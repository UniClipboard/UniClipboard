import { type ComponentProps } from 'react'
import { INVITATION_CODE_LENGTH } from '@/components/invitation-code-utils'
import { InputOTP, InputOTPGroup, InputOTPSlot } from '@/components/ui/input-otp'
import { cn } from '@/lib/utils'

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

const slotClass = cn(
  'size-10 rounded-md border border-input bg-card text-lg font-mono font-semibold uppercase shadow-xs',
  'data-[active=true]:border-primary data-[active=true]:bg-primary/5 data-[active=true]:ring-2 data-[active=true]:ring-primary/20',
  'transition-[border-color,background-color,box-shadow] duration-150'
)

const invalidSlotClass =
  'border-destructive bg-destructive/5 text-destructive data-[active=true]:border-destructive data-[active=true]:bg-destructive/5 data-[active=true]:ring-destructive/20'

export function InvitationCodeInput({
  value,
  onChange,
  disabled,
  invalid,
  className,
  ...rest
}: Props) {
  const sanitizeInvitationInput = (next: string) => {
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
  const stripInvitationCodeSeparator = (text: string) =>
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
      onChange={sanitizeInvitationInput}
      pasteTransformer={stripInvitationCodeSeparator}
      disabled={disabled}
      aria-invalid={invalid || undefined}
      containerClassName={cn('justify-center gap-3', className)}
      {...rest}
    >
      <InputOTPGroup className="gap-2 rounded-none has-aria-invalid:ring-0">
        {[0, 1, 2, 3].map(i => (
          <InputOTPSlot key={i} index={i} className={finalSlotClass} />
        ))}
      </InputOTPGroup>
      <InputOTPGroup className="gap-2 rounded-none has-aria-invalid:ring-0">
        {[4, 5, 6, 7].map(i => (
          <InputOTPSlot key={i} index={i} className={finalSlotClass} />
        ))}
      </InputOTPGroup>
    </InputOTP>
  )
}
