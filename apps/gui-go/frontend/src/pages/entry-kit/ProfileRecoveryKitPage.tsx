import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import type ExistingProfileRecoveryPage from '@/pages/ProfileRecoveryPage'
import { useProfileRecoveryForm } from '@/pages/recovery/useProfileRecoveryForm'
import { Button, EntryPage, FormField, InlineNotice } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { AdmissionRecoveryKitView } from './AdmissionRecoveryKitView'
import { EntryKitFrame } from './EntryKitFrame'

type Props = ComponentProps<typeof ExistingProfileRecoveryPage>

/**
 * Profile recovery on the platform UI kit (design R1–R3), replacing `ProfileRecoveryPage` at build
 * time through `@ui-profile-recovery-page`. Passphrase recovery and the read-only admission
 * diagnosis keep the existing state owners; this page offers no repair the daemon does not implement.
 */
export default function ProfileRecoveryPage({ status, onRecovered, onRestart }: Props) {
  const { t } = useTranslation()
  const recovery = useProfileRecoveryForm(status, onRecovered)

  if (status.state === 'admission_recovery_required') {
    return <AdmissionRecoveryKitView status={status} />
  }

  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        title={t('profileRecovery.title')}
        description={t('profileRecovery.description')}
      >
        <form id="recovery-form" onSubmit={event => void recovery.submit(event)}>
          {recovery.showLosses && (
            <InlineNotice tone="error" title={t('profileRecovery.partial')} />
          )}
          <div data-testid="recovery-passphrase">
            <FormField
              id="recovery-passphrase"
              label={t('unlock.passphraseModal.passphraseLabel')}
              type="password"
              reveal={{
                show: t('unlock.passphraseModal.show'),
                hide: t('unlock.passphraseModal.hide'),
              }}
              autoComplete="off"
              autoFocus
              value={recovery.passphrase}
              onValueChange={recovery.setPassphrase}
              disabled={recovery.inputDisabled}
              error={recovery.errorKey ? t(recovery.errorKey) : undefined}
              hint={t('profileRecovery.help')}
            />
          </div>
          {recovery.restartRequired ? (
            <Button
              variant="primary"
              size="xlarge"
              icon="retry"
              disabled={recovery.submitting}
              onClick={onRestart}
            >
              {t('profileRecovery.restart')}
            </Button>
          ) : (
            <Button
              variant="primary"
              size="xlarge"
              submitForm="recovery-form"
              busy={recovery.busy}
              disabled={!recovery.canSubmit}
            >
              {t(recovery.busy ? 'profileRecovery.recovering' : 'profileRecovery.submit')}
            </Button>
          )}
        </form>
      </EntryPage>
    </EntryKitFrame>
  )
}
