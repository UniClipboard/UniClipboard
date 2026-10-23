import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { PassphraseUnlockForm } from '@/pages/unlock/PassphraseUnlockForm'
import { useUnlockFlow } from '@/pages/unlock/useUnlockFlow'
import type { QuickPanelUnlock as KitQuickPanelUnlock } from '@/quick-panel/components/QuickPanelUnlock'

/** Platform-less builds use the same unlock flow without importing a target skin. */
export function QuickPanelUnlock({ onUnlocked }: ComponentProps<typeof KitQuickPanelUnlock>) {
  const { t } = useTranslation()
  const { needsPassphrase, unlocking, unlock, finishUnlock } = useUnlockFlow(onUnlocked)

  return (
    <section
      className="flex h-full min-h-0 flex-col overflow-y-auto p-6"
      aria-labelledby="quick-panel-unlock-title"
    >
      <div className="m-auto flex w-full max-w-sm shrink-0 flex-col gap-4">
        <h1 id="quick-panel-unlock-title" className="text-ui-title">
          {t(needsPassphrase ? 'unlock.passphraseModal.title' : 'unlock.title')}
        </h1>
        <p className="text-ui-body text-muted-foreground">
          {t(
            needsPassphrase
              ? 'unlock.passphraseModal.description'
              : 'quickPanel.history.locked.description'
          )}
        </p>
        {needsPassphrase ? (
          <PassphraseUnlockForm onUnlocked={finishUnlock} onSubmittingChange={() => {}} />
        ) : (
          <Button disabled={unlocking} onClick={() => void unlock()}>
            {t(unlocking ? 'unlock.unlocking' : 'unlock.button')}
          </Button>
        )}
      </div>
    </section>
  )
}
