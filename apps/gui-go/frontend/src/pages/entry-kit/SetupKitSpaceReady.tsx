import { useTranslation } from 'react-i18next'
import { useSpaceReadyInvite, type IssueInvitation } from '@/pages/setup/useSpaceReadyInvite'
import { Button, EntryPage, InlineNotice } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

/** S3 · the space exists; invite a device now or later. */
export function SetupKitSpaceReady({
  onInvite,
  onDone,
  loading = false,
}: {
  onInvite: IssueInvitation
  onDone: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.spaceReady' })
  const { t: tStatus } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const { inviteError, inviteRetryable, invite } = useSpaceReadyInvite(onInvite)
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: tStatus('status'), tone: 'positive', icon: 'shield' }}
        title={t('title')}
        description={t('subtitle')}
        actions={
          <>
            <span data-testid="setup-complete-later">
              <Button disabled={loading} onClick={onDone}>
                {t('actions.later')}
              </Button>
            </span>
            <span data-testid="setup-complete-invite">
              <Button
                variant="primary"
                icon="desktop"
                busy={loading}
                disabled={!inviteRetryable}
                onClick={() => void invite()}
              >
                {t(loading ? 'actions.inviting' : 'actions.invite')}
              </Button>
            </span>
          </>
        }
      >
        {inviteError ? <InlineNotice tone="error" title={inviteError} /> : null}
      </EntryPage>
    </EntryKitFrame>
  )
}
