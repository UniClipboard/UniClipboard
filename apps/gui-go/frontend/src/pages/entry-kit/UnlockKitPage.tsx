import { useState, type ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { useUnlockFlow } from '@/pages/unlock/useUnlockFlow'
import type ExistingUnlockPage from '@/pages/UnlockPage'
import { Button, ButtonRow, EntryPage } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'
import { UnlockKitPassphraseForm } from './UnlockKitPassphraseForm'
import { UnlockKitResetAlert } from './UnlockKitResetAlert'

type Props = ComponentProps<typeof ExistingUnlockPage>

/**
 * Content unlock on the platform UI kit (design U1–U3), replacing `UnlockPage` at build time
 * through `@ui-unlock-page`: the keyring first, the space passphrase when it cannot unlock, and
 * the typed "RESET" confirmation for starting over. State lives in the shared unlock hooks.
 */
export default function UnlockPage({ onUnlockSucceeded, onResetSucceeded }: Props) {
  const { t } = useTranslation()
  const { needsPassphrase, unlocking, unlock, finishUnlock } = useUnlockFlow(onUnlockSucceeded)
  const [resetOpen, setResetOpen] = useState(false)
  const [submittingPassphrase, setSubmittingPassphrase] = useState(false)

  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: t('unlock.lockedStatus'), icon: 'lock' }}
        title={t(needsPassphrase ? 'unlock.passphraseModal.title' : 'unlock.title')}
        description={t(
          needsPassphrase ? 'unlock.passphraseModal.description' : 'unlock.description'
        )}
      >
        {needsPassphrase ? (
          <UnlockKitPassphraseForm
            onUnlocked={finishUnlock}
            onSubmittingChange={setSubmittingPassphrase}
          />
        ) : (
          <div data-testid="unlock-content" className="flex flex-col">
            <Button
              variant="primary"
              size="xlarge"
              icon="unlock"
              busy={unlocking}
              onClick={() => void unlock()}
            >
              {t(unlocking ? 'unlock.unlocking' : 'unlock.button')}
            </Button>
          </div>
        )}
        <ButtonRow>
          <Button
            variant="link"
            disabled={unlocking || submittingPassphrase}
            onClick={() => setResetOpen(true)}
          >
            {t('unlock.factoryReset.link')}
          </Button>
        </ButtonRow>
      </EntryPage>
      <UnlockKitResetAlert
        open={resetOpen}
        onClose={() => setResetOpen(false)}
        onResetSucceeded={onResetSucceeded}
      />
    </EntryKitFrame>
  )
}
