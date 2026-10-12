import { useTranslation } from 'react-i18next'
import { usePassphraseUnlock } from '@/pages/unlock/usePassphraseUnlock'
import { Button, FormField } from '@/ui-kit'

/** The space passphrase, shown after the keyring could not unlock; Engine validates it. */
export function UnlockKitPassphraseForm({
  onUnlocked,
  onSubmittingChange,
}: {
  onUnlocked: () => Promise<void>
  onSubmittingChange: (submitting: boolean) => void
}) {
  const { t } = useTranslation()
  const { passphrase, setPassphrase, submitting, errorKey, submit } = usePassphraseUnlock(
    onUnlocked,
    onSubmittingChange
  )
  return (
    <form id="unlock-form" onSubmit={event => void submit(event)}>
      <div data-testid="unlock-passphrase">
        <FormField
          label={t('unlock.passphraseModal.passphraseLabel')}
          type="password"
          reveal={{
            show: t('unlock.passphraseModal.show'),
            hide: t('unlock.passphraseModal.hide'),
          }}
          autoComplete="off"
          autoFocus
          value={passphrase}
          onValueChange={setPassphrase}
          disabled={submitting}
          placeholder={t('unlock.passphraseModal.passphrasePlaceholder')}
          error={errorKey ? t(errorKey) : undefined}
          hint={t('unlock.passphraseModal.hint')}
        />
      </div>
      <Button variant="primary" size="xlarge" submitForm="unlock-form" busy={submitting}>
        {t(submitting ? 'unlock.passphraseModal.submitting' : 'unlock.passphraseModal.submit')}
      </Button>
    </form>
  )
}
