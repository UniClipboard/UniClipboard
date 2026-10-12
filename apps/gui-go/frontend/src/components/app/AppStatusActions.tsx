import { ArrowUpCircle, Check, Download, Loader2, MessageCircle, RotateCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { STARTUP_SUPPORT_URL } from '@/api/startup-support'
import { Button } from '@/components/ui/button'
import { useStatusActions } from './useStatusActions'

type Props = {
  onRetry: () => void
  retrying: boolean
  versionTooOld: boolean
}

export function AppStatusActions({ onRetry, retrying, versionTooOld }: Props) {
  const { t } = useTranslation()
  const { action, feedback, runAction } = useStatusActions()

  return (
    <>
      <div className="mt-7 flex flex-wrap gap-2">
        {!versionTooOld && (
          <Button disabled={retrying || action !== null} onClick={onRetry}>
            {retrying ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <RotateCw className="size-4" />
            )}
            {t(retrying ? 'startupFailure.retrying' : 'startupFailure.retry')}
          </Button>
        )}
        <Button
          variant={versionTooOld ? 'default' : 'outline'}
          disabled={action !== null || retrying}
          onClick={() => void runAction('update')}
        >
          {action === 'update' ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <ArrowUpCircle className="size-4" />
          )}
          {t(versionTooOld ? 'startupFailure.update' : 'settings.sections.about.checkUpdate')}
        </Button>
        <Button variant="ghost" disabled={action !== null} onClick={() => void runAction('export')}>
          {action === 'export' ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Download className="size-4" />
          )}
          {t(action === 'export' ? 'startupFailure.exporting' : 'startupFailure.export')}
        </Button>
        <Button
          variant="ghost"
          disabled={action !== null}
          onClick={() => void runAction('contact')}
        >
          <MessageCircle className="size-4" />
          {t('startupFailure.contact')}
        </Button>
      </div>
      {feedback && (
        <div
          role={feedback.error ? 'alert' : 'status'}
          className="mt-4 flex items-start gap-2 break-words rounded-lg border border-border px-3 py-2.5 text-ui-body [overflow-wrap:anywhere]"
        >
          {!feedback.error && (
            <Check
              className="mt-0.5 size-4 shrink-0 text-emerald-700 dark:text-emerald-400"
              aria-hidden="true"
            />
          )}
          <div className="min-w-0">
            {feedback.message}
            {feedback.error && (
              <p className="mt-1 select-text text-muted-foreground">{STARTUP_SUPPORT_URL}</p>
            )}
          </div>
        </div>
      )}
    </>
  )
}
