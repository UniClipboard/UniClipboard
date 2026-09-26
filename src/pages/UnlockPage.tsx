import { Loader2, Lock, LockOpen } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AppStateShell } from '@/components/app/AppStateShell'
import { Button } from '@/components/ui/button'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'
import { FactoryResetDialog } from './unlock/FactoryResetDialog'
import { PassphraseUnlockForm } from './unlock/PassphraseUnlockForm'

const log = createLogger('unlock-page')

interface UnlockPageProps {
  onUnlockSucceeded?: () => void
  onResetSucceeded?: () => void
}

export default function UnlockPage({ onUnlockSucceeded, onResetSucceeded }: UnlockPageProps) {
  const { t } = useTranslation()
  const [needsPassphrase, setNeedsPassphrase] = useState(false)
  const [resetOpen, setResetOpen] = useState(false)
  const [unlocking, setUnlocking] = useState(false)
  const [submittingPassphrase, setSubmittingPassphrase] = useState(false)
  const resetLinkRef = useRef<HTMLButtonElement>(null)

  const finishUnlock = async () => {
    try {
      await ensureSetupRealtimeSync()
      await refreshSetupState()
    } catch (error) {
      log.warn({ err: error }, 'Setup state refresh failed after unlock')
    }
    onUnlockSucceeded?.()
  }

  const unlock = async () => {
    if (unlocking) return
    setUnlocking(true)
    try {
      if (await commands.unlockContentFromKeyring()) {
        await finishUnlock()
        return
      }
    } catch (error) {
      log.warn({ err: error }, 'Keyring unlock failed; requesting the passphrase')
    } finally {
      setUnlocking(false)
    }
    setNeedsPassphrase(true)
  }

  return (
    <>
      <AppStateShell
        category={t('unlock.lockedStatus')}
        categoryIcon={<Lock className="size-4" aria-hidden="true" />}
        title={t(needsPassphrase ? 'unlock.passphraseModal.title' : 'unlock.title')}
        description={t(
          needsPassphrase ? 'unlock.passphraseModal.description' : 'unlock.description'
        )}
      >
        {needsPassphrase ? (
          <PassphraseUnlockForm
            onUnlocked={finishUnlock}
            onSubmittingChange={setSubmittingPassphrase}
          />
        ) : (
          <Button
            data-testid="unlock-content"
            size="lg"
            className="mt-7 w-full"
            onClick={() => void unlock()}
            disabled={unlocking}
            aria-busy={unlocking}
          >
            {unlocking ? (
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            ) : (
              <LockOpen className="size-4" aria-hidden="true" />
            )}
            {unlocking ? t('unlock.unlocking') : t('unlock.button')}
          </Button>
        )}

        <div className="mt-8 border-t border-border pt-4">
          <button
            ref={resetLinkRef}
            type="button"
            onClick={() => setResetOpen(true)}
            disabled={unlocking || submittingPassphrase}
            className="rounded-sm text-ui-body text-muted-foreground underline decoration-border underline-offset-4 transition-colors outline-none hover:text-foreground hover:decoration-current focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
          >
            {t('unlock.factoryReset.link')}
          </button>
        </div>
      </AppStateShell>

      <FactoryResetDialog
        open={resetOpen}
        onClose={() => setResetOpen(false)}
        onResetSucceeded={onResetSucceeded}
        finalFocus={resetLinkRef}
      />
    </>
  )
}
