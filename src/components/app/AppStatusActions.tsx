import { ArrowUpCircle, Download, Loader2, MessageCircle, RotateCw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { contactAuthor, exportStartupLogs, STARTUP_SUPPORT_URL } from '@/api/startup-support'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { Button } from '@/components/ui/button'

type StatusAction = 'export' | 'contact' | 'update'

type Props = {
  onRetry: () => void
  retrying: boolean
  versionTooOld: boolean
}

async function performStatusAction(next: StatusAction) {
  if (next === 'export') return exportStartupLogs()
  if (next === 'contact') {
    await contactAuthor()
    return null
  }
  await openUpdaterWindow()
  await checkForUpdate(null)
  return null
}

export function AppStatusActions({ onRetry, retrying, versionTooOld }: Props) {
  const { t } = useTranslation()
  const [action, setAction] = useState<StatusAction | null>(null)
  const [feedback, setFeedback] = useState<{ error: boolean; message: string } | null>(null)

  async function runAction(next: StatusAction) {
    if (action) return
    setAction(next)
    setFeedback(null)
    try {
      const path = await performStatusAction(next)
      if (next === 'export' && path)
        setFeedback({ error: false, message: t('startupFailure.exported', { path }) })
      if (next === 'contact')
        setFeedback({ error: false, message: t('startupFailure.contactOpened') })
    } catch {
      setFeedback({ error: true, message: t(`startupFailure.${next}Failed`) })
    } finally {
      setAction(null)
    }
  }

  return (
    <>
      <div className="mt-7 flex flex-wrap gap-3">
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
        <Button
          variant="outline"
          disabled={action !== null}
          onClick={() => void runAction('export')}
        >
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
          className="mt-5 break-words rounded-md border border-border p-3 text-ui-body [overflow-wrap:anywhere]"
        >
          {feedback.message}
          {feedback.error && (
            <p className="mt-1 select-text text-muted-foreground">{STARTUP_SUPPORT_URL}</p>
          )}
        </div>
      )}
    </>
  )
}
