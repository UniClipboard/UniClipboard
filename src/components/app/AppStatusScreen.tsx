import {
  AlertCircle,
  ArrowUpCircle,
  Download,
  Loader2,
  MessageCircle,
  RotateCw,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { contactAuthor, exportStartupLogs, STARTUP_SUPPORT_URL } from '@/api/startup-support'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { Button } from '@/components/ui/button'
import type { DaemonBootstrapFailure } from '@/lib/ipc'
import appIcon from '@/updater/app-icon.png'

type AppStatusScreenProps = {
  detail: string | undefined | null
  failure?: DaemonBootstrapFailure | null
  onRetry: () => void
  retrying?: boolean
}

export function AppStatusScreen({
  detail,
  failure,
  onRetry,
  retrying = false,
}: AppStatusScreenProps) {
  const { t } = useTranslation()
  const [action, setAction] = useState<'export' | 'contact' | 'update' | null>(null)
  const [feedback, setFeedback] = useState<{ error: boolean; message: string } | null>(null)
  const versionTooOld = failure?.kind === 'versionTooOld'

  async function runAction(next: 'export' | 'contact' | 'update') {
    if (action) return
    setAction(next)
    setFeedback(null)
    try {
      if (next === 'export') {
        const path = await exportStartupLogs()
        if (path) setFeedback({ error: false, message: t('startupFailure.exported', { path }) })
      } else if (next === 'contact') {
        await contactAuthor()
        setFeedback({ error: false, message: t('startupFailure.contactOpened') })
      } else {
        await openUpdaterWindow()
        await checkForUpdate(null)
      }
    } catch {
      setFeedback({ error: true, message: t(`startupFailure.${next}Failed`) })
    } finally {
      setAction(null)
    }
  }

  return (
    <main className="min-h-0 flex-1 overflow-y-auto bg-background text-foreground">
      <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col justify-center px-6 py-10 sm:px-10">
        <div className="mb-10 flex items-center gap-3">
          <img src={appIcon} alt="" className="size-10 shrink-0" />
          <span className="text-base font-semibold">UniClipboard</span>
        </div>
        <div className="mb-4 flex items-center gap-2 text-sm font-medium text-destructive">
          <AlertCircle className="size-4" aria-hidden="true" />
          {t('startupFailure.unavailable')}
        </div>
        <h1 className="text-2xl font-semibold leading-tight">
          {t(versionTooOld ? 'startupFailure.updateTitle' : 'startupFailure.title')}
        </h1>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          {t(versionTooOld ? 'startupFailure.updateDescription' : 'startupFailure.description')}
        </p>
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
            className="mt-5 break-words rounded-md border border-border p-3 text-sm leading-6 [overflow-wrap:anywhere]"
          >
            {feedback.message}
            {feedback.error && (
              <p className="mt-1 select-text text-muted-foreground">{STARTUP_SUPPORT_URL}</p>
            )}
          </div>
        )}
        {detail && (
          <details className="mt-8 border-t border-border pt-4 text-sm">
            <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
              {t('startupFailure.details')}
            </summary>
            <pre className="mt-3 max-h-40 select-text overflow-y-auto whitespace-pre-wrap break-words font-mono text-xs leading-5 text-muted-foreground [overflow-wrap:anywhere]">
              {detail}
            </pre>
          </details>
        )}
      </div>
    </main>
  )
}
