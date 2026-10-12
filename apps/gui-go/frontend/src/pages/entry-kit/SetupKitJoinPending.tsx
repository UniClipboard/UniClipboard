import { useTranslation } from 'react-i18next'
import { Button, EntryPage, ProgressBar } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

/** S7 · waiting for the sponsor's final confirmation; once processing, the join cannot be cancelled. */
export function SetupKitJoinPending({
  status,
  onCancel,
  loading = false,
}: {
  status: 'pending' | 'processing'
  onCancel: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinPending' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  const processing = status === 'processing'
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: tWelcome('join.cta'), busy: true }}
        title={t(processing ? 'processing.title' : 'title')}
        description={t(processing ? 'processing.subtitle' : 'subtitle')}
        actions={
          processing ? undefined : (
            <span data-testid="setup-join-cancel">
              <Button busy={loading} onClick={onCancel}>
                {t('actions.cancel')}
              </Button>
            </span>
          )
        }
      >
        <div data-testid={processing ? 'setup-join-processing' : 'setup-join-pending'}>
          <ProgressBar label={t(processing ? 'processing.title' : 'title')} />
        </div>
      </EntryPage>
    </EntryKitFrame>
  )
}
