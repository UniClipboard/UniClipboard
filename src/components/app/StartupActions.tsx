import { ArrowUpCircle, Download, Loader2, MessageCircle, RotateCw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { contactAuthor, STARTUP_SUPPORT_URL } from '@/api/startup-support'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { Button } from '@/components/ui/button'
import type { StartupSnapshot } from '@/lib/startup-progress'

type Props = {
  failed: boolean
  onExport: () => Promise<void | boolean> | void
  onRetry: () => void
  required: boolean
  snapshot: StartupSnapshot
}

export function StartupActions({ failed, onExport, onRetry, required, snapshot }: Props) {
  const { t } = useTranslation()
  const [exportState, setExportState] = useState<'idle' | 'working' | 'failed' | 'done'>('idle')
  const [contactState, setContactState] = useState<'idle' | 'working' | 'failed' | 'done'>('idle')
  const [updateState, setUpdateState] = useState<'idle' | 'working' | 'failed'>('idle')

  async function exportLogs() {
    if (exportState === 'working') return
    setExportState('working')
    try {
      const exported = await onExport()
      setExportState(exported === false ? 'idle' : 'done')
    } catch {
      setExportState('failed')
    }
  }

  async function openSupport() {
    if (contactState === 'working') return
    setContactState('working')
    try {
      await contactAuthor()
      setContactState('done')
    } catch {
      setContactState('failed')
    }
  }

  async function checkUpdate() {
    if (updateState === 'working') return
    setUpdateState('working')
    try {
      await openUpdaterWindow()
      await checkForUpdate(null)
      setUpdateState('idle')
    } catch {
      setUpdateState('failed')
    }
  }

  return (
    <>
      <div className="mt-8 flex flex-wrap gap-3">
        {snapshot.allowed_actions.retry && failed && (
          <Button onClick={onRetry}>
            <RotateCw className="size-4" />
            {t(required ? 'upgradeProgress.retry' : 'startupFailure.retry')}
          </Button>
        )}
        {failed && (
          <Button
            variant="outline"
            disabled={updateState === 'working'}
            onClick={() => void checkUpdate()}
          >
            {updateState === 'working' ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <ArrowUpCircle className="size-4" />
            )}
            {t('settings.sections.about.checkUpdate')}
          </Button>
        )}
        {snapshot.allowed_actions.export_diagnostics && (
          <Button
            variant="ghost"
            disabled={exportState === 'working'}
            onClick={() => void exportLogs()}
          >
            <Download className="size-4" />
            {t('upgradeProgress.export')}
          </Button>
        )}
        {failed && (
          <Button
            variant="ghost"
            disabled={contactState === 'working'}
            onClick={() => void openSupport()}
          >
            {contactState === 'working' ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <MessageCircle className="size-4" />
            )}
            {t('startupFailure.contact')}
          </Button>
        )}
      </div>
      {exportState === 'failed' && (
        <p role="alert" className="mt-3 text-ui-body text-destructive">
          {t('upgradeProgress.exportFailed')}
        </p>
      )}
      {exportState === 'done' && (
        <p role="status" className="mt-3 text-ui-body text-muted-foreground">
          {t('upgradeProgress.exportDone')}
        </p>
      )}
      {contactState === 'failed' && (
        <div
          role="alert"
          className="mt-3 break-words rounded-md border border-border p-3 text-ui-body [overflow-wrap:anywhere]"
        >
          {t('startupFailure.contactFailed')}
          <p className="mt-1 select-text text-muted-foreground">{STARTUP_SUPPORT_URL}</p>
        </div>
      )}
      {contactState === 'done' && (
        <p role="status" className="mt-3 text-ui-body text-muted-foreground">
          {t('startupFailure.contactOpened')}
        </p>
      )}
      {updateState === 'failed' && (
        <p role="alert" className="mt-3 text-ui-body text-destructive">
          {t('startupFailure.updateFailed')}
        </p>
      )}
    </>
  )
}
