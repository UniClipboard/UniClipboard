import { useTranslation } from 'react-i18next'
import { useInvitationCountdown } from '@/pages/setup/useInvitationCountdown'
import { Button, EntryPage, ProgressBar, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

/** S4/S5 · the invitation code with its countdown; it is cancelled automatically when it expires. */
export function SetupKitInvitation({
  code,
  expiresAtMs,
  onCancel,
  loading,
}: {
  code: string
  expiresAtMs: number
  onCancel: () => void
  loading?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const { display, expired, progress, remainingLabel } = useInvitationCountdown(
    code,
    expiresAtMs,
    onCancel
  )
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: t('status'), tone: 'positive', icon: 'shield' }}
        title={t('title')}
        description={t('subtitle')}
        actions={
          <span data-testid="setup-invitation-cancel">
            <Button busy={loading} onClick={onCancel}>
              {t('actions.cancel')}
            </Button>
          </span>
        }
      >
        <div className="flex flex-col gap-3">
          <div
            data-testid="setup-invitation-code"
            className={`select-all break-words ${expired ? 'line-through opacity-50' : ''}`}
          >
            <Text role="display" monospace>
              {display}
            </Text>
          </div>
          <ProgressBar label={t('expiresIn', { remaining: remainingLabel })} value={progress} />
          <span role="timer">
            <Text role="caption" tone={expired ? 'critical' : 'secondary'}>
              {expired ? t('expired') : t('expiresIn', { remaining: remainingLabel })}
            </Text>
          </span>
          <Text role="caption" tone="secondary">
            {expired ? t('hintExpired') : t('hint')}
          </Text>
        </div>
      </EntryPage>
    </EntryKitFrame>
  )
}
