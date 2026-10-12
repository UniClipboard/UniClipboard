import { useTranslation } from 'react-i18next'
import type { JoinSpaceEndReason } from '@/api/daemon/setupV2'
import { Button, EntryPage, InlineNotice } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

/** S8 · the join ended without activation; the reason comes from the daemon's admission outcome. */
export function SetupKitJoinEnded({
  reason,
  onBack,
}: {
  reason: JoinSpaceEndReason
  onBack: () => void
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinPending' })
  const { t: tWelcome } = useTranslation(undefined, { keyPrefix: 'setup.welcome' })
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: tWelcome('join.cta'), tone: 'critical', icon: 'info' }}
        title={t('ended.title')}
        actions={
          <Button data-testid="setup-join-ended-back" variant="primary" onClick={onBack}>
            {t('actions.back')}
          </Button>
        }
      >
        <div data-testid="setup-join-ended">
          <InlineNotice tone="error" title={t(`ended.reasons.${reason}`)} />
        </div>
      </EntryPage>
    </EntryKitFrame>
  )
}
