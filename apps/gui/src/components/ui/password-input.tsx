import { Eye, EyeOff } from 'lucide-react'
import { useState, type ComponentProps, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

/** Text input for secrets with an accessible show/hide toggle. */
export function PasswordInput({
  className,
  disabled,
  onKeyDown,
  ...props
}: ComponentProps<typeof Input>) {
  const { t } = useTranslation()
  const [visible, setVisible] = useState(false)

  // Submit the owning form explicitly on Enter. Implicit form submission only
  // follows trusted key presses, so synthetic Enter events (assistive tools and
  // the WebDriver used by desktop E2E) would otherwise do nothing.
  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event)
    if (event.defaultPrevented || event.key !== 'Enter' || event.nativeEvent.isComposing) return
    const form = event.currentTarget.form
    if (!form) return
    event.preventDefault()
    form.requestSubmit()
  }

  return (
    <div className="relative">
      <Input
        {...props}
        type={visible ? 'text' : 'password'}
        autoComplete="off"
        disabled={disabled}
        onKeyDown={submitOnEnter}
        className={cn('pr-10', className)}
      />
      <button
        type="button"
        onClick={() => setVisible(value => !value)}
        disabled={disabled}
        aria-label={t(visible ? 'unlock.passphraseModal.hide' : 'unlock.passphraseModal.show')}
        aria-pressed={visible}
        className="absolute inset-y-0 right-0 flex w-9 items-center justify-center rounded-r-lg text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
      >
        {visible ? (
          <EyeOff className="size-4" aria-hidden="true" />
        ) : (
          <Eye className="size-4" aria-hidden="true" />
        )}
      </button>
    </div>
  )
}
