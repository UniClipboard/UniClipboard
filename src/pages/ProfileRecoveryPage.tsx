import { Download, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ProfileRecoveryResponse } from '@/api/daemon/encryption'
import { exportStartupLogs } from '@/api/startup-support'
import { Button } from '@/components/ui/button'
import appIcon from '@/updater/app-icon.png'

export default function ProfileRecoveryPage({ status }: { status: ProfileRecoveryResponse }) {
  const { t } = useTranslation()
  const [exportState, setExportState] = useState<'idle' | 'exporting' | 'done' | 'failed'>('idle')
  const admission = status.admission

  async function exportDiagnostics() {
    if (exportState === 'exporting') return
    setExportState('exporting')
    try {
      const path = await exportStartupLogs()
      setExportState(path ? 'done' : 'idle')
    } catch {
      setExportState('failed')
    }
  }

  return (
    <main className="min-h-0 flex-1 overflow-y-auto bg-background text-foreground">
      <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col justify-center px-6 py-10 sm:px-10">
        <div className="mb-10 flex items-center gap-3">
          <img src={appIcon} alt="" className="size-10 shrink-0" />
          <span className="text-ui-section font-semibold">UniClipboard</span>
        </div>
        <div className="mb-4 flex items-center gap-2 text-ui-body font-medium text-destructive">
          <TriangleAlert className="size-4" aria-hidden="true" />
          {t('profileAdmissionRecovery.category')}
        </div>
        <h1 className="text-ui-title font-semibold">{t('profileAdmissionRecovery.title')}</h1>
        <p className="mt-3 text-ui-body text-muted-foreground">
          {t('profileAdmissionRecovery.description')}
        </p>
        {admission && (
          <dl className="mt-7 grid grid-cols-[auto_1fr] gap-x-5 gap-y-3 border-y border-border py-4 text-ui-body">
            <dt className="text-muted-foreground">{t('profileAdmissionRecovery.issue')}</dt>
            <dd>{t(`profileAdmissionRecovery.categories.${admission.category}`)}</dd>
            <dt className="text-muted-foreground">{t('profileAdmissionRecovery.stage')}</dt>
            <dd>{t(`profileAdmissionRecovery.stages.${admission.stage}`)}</dd>
            <dt className="text-muted-foreground">{t('profileAdmissionRecovery.nextStep')}</dt>
            <dd>{t(`profileAdmissionRecovery.actions.${admission.action}`)}</dd>
          </dl>
        )}
        <p className="mt-5 text-ui-body font-medium">{t('profileAdmissionRecovery.preserved')}</p>
        <p className="mt-2 text-ui-caption text-muted-foreground">
          {t('profileAdmissionRecovery.help')}
        </p>
        <div className="mt-6">
          <Button
            type="button"
            variant="outline"
            disabled={exportState === 'exporting'}
            onClick={() => void exportDiagnostics()}
          >
            <Download data-icon="inline-start" aria-hidden="true" />
            {t(
              exportState === 'exporting'
                ? 'profileAdmissionRecovery.exporting'
                : 'profileAdmissionRecovery.export'
            )}
          </Button>
        </div>
        {exportState === 'done' && (
          <p role="status" className="mt-3 text-ui-caption text-emerald-600 dark:text-emerald-400">
            {t('profileAdmissionRecovery.exported')}
          </p>
        )}
        {exportState === 'failed' && (
          <p role="alert" className="mt-3 text-ui-caption text-destructive">
            {t('profileAdmissionRecovery.exportFailed')}
          </p>
        )}
      </div>
    </main>
  )
}
