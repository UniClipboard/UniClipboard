import { Loader2, Unlock } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AppStateShell } from '@/components/app/AppStateShell'
import { Button } from '@/components/ui/button'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'
import { FactoryResetDialog } from './unlock/FactoryResetDialog'
import { PassphraseUnlockDialog } from './unlock/PassphraseUnlockDialog'

const log = createLogger('unlock-page')

interface UnlockPageProps {
  onUnlockSucceeded?: () => void
  onResetSucceeded?: () => void
}

type OpenDialog = 'passphrase' | 'reset' | null

export default function UnlockPage({ onUnlockSucceeded, onResetSucceeded }: UnlockPageProps) {
  const { t } = useTranslation()
  const [openDialog, setOpenDialog] = useState<OpenDialog>(null)
  const [unlocking, setUnlocking] = useState(false)

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
    setOpenDialog('passphrase')
  }

  return (
    <>
      <AppStateShell
        title={t('unlock.title')}
        description={t('unlock.description')}
        width="compact"
      >
        <div className="mt-7">
          <Button
            data-testid="unlock-content"
            className="w-full"
            onClick={() => void unlock()}
            disabled={unlocking}
          >
            {unlocking ? (
              <Loader2 className="mr-2 size-5 animate-spin" />
            ) : (
              <Unlock className="mr-2 size-5" />
            )}
            {unlocking ? t('unlock.unlocking') : t('unlock.button')}
          </Button>

          <button
            type="button"
            onClick={() => setOpenDialog('reset')}
            className="mt-8 text-ui-caption text-muted-foreground/70 underline-offset-4 transition-colors hover:text-muted-foreground hover:underline"
          >
            {t('unlock.factoryReset.link')}
          </button>
        </div>
      </AppStateShell>

      <PassphraseUnlockDialog
        open={openDialog === 'passphrase'}
        onClose={() => setOpenDialog(null)}
        onResetRequested={() => setOpenDialog('reset')}
        onUnlocked={finishUnlock}
      />
      <FactoryResetDialog
        open={openDialog === 'reset'}
        onClose={() => setOpenDialog(null)}
        onResetSucceeded={onResetSucceeded}
      />
    </>
  )
}
