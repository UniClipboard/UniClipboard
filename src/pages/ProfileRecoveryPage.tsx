import { Download, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { exportStartupLogs } from '@/api/startup-support'
import { AppStateShell } from '@/components/app/AppStateShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'
import type { ProfileRecoveryResponse } from '@/lib/ipc-bindings.generated'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('profile-recovery-page')

export default function ProfileRecoveryPage({
  status,
  onRecovered,
  onRestart,
}: {
  status: ProfileRecoveryResponse
  onRecovered: () => void
  onRestart: () => void
}) {
  const { t } = useTranslation()
  const [form, setForm] = useState({ passphrase: '', submitting: false, errorKey: '' })

  if (status.state === 'admission_recovery_required') {
    return <AdmissionRecoveryView status={status} />
  }

  const busy = form.submitting || status.state === 'recovering'
  const statusErrorKey =
    status.state === 'partially_recoverable'
      ? 'profileRecovery.partial'
      : status.state === 'failed'
        ? 'profileRecovery.failed'
        : ''
  const errorKey = status.restartRequired
    ? 'profileRecovery.restartRequired'
    : form.errorKey || statusErrorKey
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (busy || status.restartRequired || !status.canSubmitPassphrase || !form.passphrase) return
    setForm(value => ({ ...value, submitting: true, errorKey: '' }))
    try {
      await commands.unlockContent({ passphrase: form.passphrase })
      setForm({ passphrase: '', submitting: false, errorKey: '' })
      try {
        await ensureSetupRealtimeSync()
        await refreshSetupState()
      } catch {
        log.warn('Setup refresh pending after profile recovery')
      }
      onRecovered()
    } catch (error) {
      setForm(value => ({ ...value, submitting: false, errorKey: contentUnlockErrorKey(error) }))
    }
  }
  return (
    <AppStateShell
      title={t('profileRecovery.title')}
      description={t('profileRecovery.description')}
      width="compact"
    >
      <form onSubmit={submit} className="mt-7 flex flex-col gap-4">
        {status.losses.length > 0 && !errorKey && (
          <p
            role="alert"
            className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-ui-body text-destructive"
          >
            {t('profileRecovery.partial')}
          </p>
        )}
        <Label htmlFor="recovery-passphrase">{t('unlock.passphraseModal.passphraseLabel')}</Label>
        <Input
          id="recovery-passphrase"
          type="password"
          autoComplete="off"
          autoFocus
          value={form.passphrase}
          disabled={busy || status.restartRequired || !status.canSubmitPassphrase}
          onChange={event =>
            setForm(value => ({ ...value, passphrase: event.target.value, errorKey: '' }))
          }
          aria-invalid={Boolean(form.errorKey)}
          aria-describedby="recovery-help"
        />
        {errorKey && (
          <p role="alert" className="text-ui-body text-destructive">
            {t(errorKey)}
          </p>
        )}
        {status.restartRequired ? (
          <Button type="button" onClick={onRestart} disabled={form.submitting}>
            {t('profileRecovery.restart')}
          </Button>
        ) : (
          <Button type="submit" disabled={busy || !status.canSubmitPassphrase || !form.passphrase}>
            {t(busy ? 'profileRecovery.recovering' : 'profileRecovery.submit')}
          </Button>
        )}
        <p id="recovery-help" className="text-ui-caption text-muted-foreground">
          {t('profileRecovery.help')}
        </p>
      </form>
    </AppStateShell>
  )
}

function AdmissionRecoveryView({ status }: { status: ProfileRecoveryResponse }) {
  const { t } = useTranslation()
  const [exportState, setExportState] = useState<'idle' | 'exporting' | 'done' | 'failed'>('idle')
  const admission = status.admission
  const exportDiagnostics = async () => {
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
    <AppStateShell
      title={t('profileAdmissionRecovery.title')}
      description={t('profileAdmissionRecovery.description')}
      width="compact"
    >
      <div className="mt-6 flex items-center gap-2 text-ui-body font-medium text-destructive">
        <TriangleAlert className="size-4" aria-hidden="true" />
        {t('profileAdmissionRecovery.category')}
      </div>
      {admission && (
        <dl className="mt-5 grid grid-cols-[auto_1fr] gap-x-5 gap-y-3 border-y border-border py-4 text-ui-body">
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
      <Button
        type="button"
        variant="outline"
        className="mt-6"
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
    </AppStateShell>
  )
}
