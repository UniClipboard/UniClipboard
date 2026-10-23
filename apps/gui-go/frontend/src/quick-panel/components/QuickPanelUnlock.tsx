import { useTranslation } from 'react-i18next'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { UnlockKitPassphraseForm } from '@/pages/entry-kit/UnlockKitPassphraseForm'
import { useUnlockFlow } from '@/pages/unlock/useUnlockFlow'
import { Button, Text, UiRoot } from '@/ui-kit'

/** Unlock in this WebView; the daemon remains the authority for content access. */
export function QuickPanelUnlock({ onUnlocked }: { onUnlocked: () => void }) {
  const { t } = useTranslation()
  const colorScheme = useAppliedColorScheme()
  const { needsPassphrase, unlocking, unlock, finishUnlock } = useUnlockFlow(onUnlocked)

  return (
    <UiRoot colorScheme={colorScheme}>
      <section
        className="flex h-full min-h-0 flex-col overflow-y-auto p-6"
        aria-labelledby="quick-panel-unlock-title"
      >
        <div className="m-auto flex w-full max-w-sm shrink-0 flex-col gap-4">
          <h1 id="quick-panel-unlock-title" className="text-ui-title">
            {t(needsPassphrase ? 'unlock.passphraseModal.title' : 'unlock.title')}
          </h1>
          <Text tone="secondary">
            {t(
              needsPassphrase
                ? 'unlock.passphraseModal.description'
                : 'quickPanel.history.locked.description'
            )}
          </Text>
          {needsPassphrase ? (
            <UnlockKitPassphraseForm onUnlocked={finishUnlock} onSubmittingChange={() => {}} />
          ) : (
            <Button variant="primary" icon="unlock" busy={unlocking} onClick={() => void unlock()}>
              {t(unlocking ? 'unlock.unlocking' : 'unlock.button')}
            </Button>
          )}
        </div>
      </section>
    </UiRoot>
  )
}
