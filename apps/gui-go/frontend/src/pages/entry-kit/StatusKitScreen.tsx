import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { STARTUP_SUPPORT_URL } from '@/api/startup-support'
import type { AppStatusScreen as ExistingAppStatusScreen } from '@/components/app/AppStatusScreen'
import { useStatusActions } from '@/components/app/useStatusActions'
import { Button, ButtonRow, Disclosure, EntryPage, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFrame } from './EntryKitFrame'

type Props = ComponentProps<typeof ExistingAppStatusScreen>

/**
 * The background-service failure screen on the platform UI kit (design T4, T6), replacing
 * `AppStatusScreen` at build time through `@ui-app-status-screen` with the same props and actions.
 */
export function AppStatusScreen({ detail, failure, onRetry, retrying = false }: Props) {
  const { t } = useTranslation()
  const { action, feedback, runAction } = useStatusActions()
  const versionTooOld = failure?.kind === 'versionTooOld'

  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: t('startupFailure.unavailable'), tone: 'critical', icon: 'info' }}
        title={t(versionTooOld ? 'startupFailure.updateTitle' : 'startupFailure.title')}
        description={t(
          versionTooOld ? 'startupFailure.updateDescription' : 'startupFailure.description'
        )}
      >
        <ButtonRow>
          {!versionTooOld && (
            <Button
              variant="primary"
              icon="retry"
              busy={retrying}
              disabled={action !== null}
              onClick={onRetry}
            >
              {t(retrying ? 'startupFailure.retrying' : 'startupFailure.retry')}
            </Button>
          )}
          <Button
            variant={versionTooOld ? 'primary' : 'standard'}
            icon="update"
            busy={action === 'update'}
            disabled={action !== null || retrying}
            onClick={() => void runAction('update')}
          >
            {t(versionTooOld ? 'startupFailure.update' : 'settings.sections.about.checkUpdate')}
          </Button>
          <Button
            icon="download"
            busy={action === 'export'}
            disabled={action !== null}
            onClick={() => void runAction('export')}
          >
            {t(action === 'export' ? 'startupFailure.exporting' : 'startupFailure.export')}
          </Button>
          <Button
            variant="link"
            disabled={action !== null}
            onClick={() => void runAction('contact')}
          >
            {t('startupFailure.contact')}
          </Button>
        </ButtonRow>
        {feedback && (
          <p
            role={feedback.error ? 'alert' : 'status'}
            className="m-0 flex flex-col gap-1 break-words"
          >
            <Text tone={feedback.error ? 'critical' : 'positive'}>{feedback.message}</Text>
            {feedback.error && <Text tone="secondary">{STARTUP_SUPPORT_URL}</Text>}
          </p>
        )}
        {detail && (
          <Disclosure label={t('startupFailure.details')}>
            <Text role="caption" tone="secondary" monospace>
              <span className="select-text whitespace-pre-wrap break-words">{detail}</span>
            </Text>
          </Disclosure>
        )}
      </EntryPage>
    </EntryKitFrame>
  )
}
