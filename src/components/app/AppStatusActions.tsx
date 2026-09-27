import { ArrowUpCircle, Check, Download, Loader2, MessageCircle, RotateCw } from 'lucide-react'
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
