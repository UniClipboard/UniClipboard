import { AlertCircle, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { PasswordInput } from '@/components/ui/password-input'
import { usePassphraseUnlock } from './usePassphraseUnlock'

interface PassphraseUnlockFormProps {
  onUnlocked: () => Promise<void>
  onSubmittingChange: (submitting: boolean) => void
}

/** Inline passphrase fallback shown after the explicit keyring attempt fails. */
export function PassphraseUnlockForm({
  onUnlocked,
  onSubmittingChange,
}: PassphraseUnlockFormProps) {
  const { t } = useTranslation()
  const { passphrase, setPassphrase, submitting, errorKey, submit } = usePassphraseUnlock(
    onUnlocked,
    onSubmittingChange
  )

  return (
    <form onSubmit={event => void submit(event)} className="mt-7 flex flex-col gap-2">
      <Label htmlFor="unlock-passphrase">{t('unlock.passphraseModal.passphraseLabel')}</Label>
      <PasswordInput
        id="unlock-passphrase"
        value={passphrase}
        onChange={event => setPassphrase(event.target.value)}
        disabled={submitting}
        placeholder={t('unlock.passphraseModal.passphrasePlaceholder')}
        autoFocus
        aria-invalid={errorKey !== null}
        aria-describedby={errorKey ? 'unlock-error unlock-hint' : 'unlock-hint'}
      />
      {errorKey && (
        <p
          id="unlock-error"
          role="alert"
          className="flex items-start gap-1.5 text-ui-body font-medium text-destructive"
        >
          <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          {t(errorKey)}
        </p>
      )}
      <p id="unlock-hint" className="text-ui-caption-relaxed text-muted-foreground">
        {t('unlock.passphraseModal.hint')}
      </p>
      <Button type="submit" size="lg" className="mt-3 w-full" disabled={submitting}>
        {submitting && <Loader2 className="size-4 animate-spin" aria-hidden="true" />}
        {t(submitting ? 'unlock.passphraseModal.submitting' : 'unlock.passphraseModal.submit')}
      </Button>
    </form>
  )
}
