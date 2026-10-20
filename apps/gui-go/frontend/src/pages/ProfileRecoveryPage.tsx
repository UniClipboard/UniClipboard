import { Download, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ProfileRecoveryResponse } from '@/api/generated/types.gen'
import { AppStateShell } from '@/components/app/AppStateShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useDiagnosticsExport } from '@/hooks/useDiagnosticsExport'
import { useProfileRecoveryForm } from './recovery/useProfileRecoveryForm'

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
  const recovery = useProfileRecoveryForm(status, onRecovered)

  if (status.state === 'admission_recovery_required' || recovery.requiresSupport) {
    return <ProfileRecoverySupportView status={status} />
  }

  const { errorKey } = recovery
  return (
    <AppStateShell
      title={t('profileRecovery.title')}
      description={t('profileRecovery.description')}
    >
      <form onSubmit={event => void recovery.submit(event)} className="mt-7 flex flex-col gap-4">
        {recovery.showLosses && (
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
          value={recovery.passphrase}
          disabled={recovery.inputDisabled}
          onChange={event => recovery.setPassphrase(event.target.value)}
          aria-invalid={Boolean(recovery.submitErrorKey)}
          aria-describedby="recovery-help"
        />
        {errorKey && (
          <p role="alert" className="text-ui-body text-destructive">
            {t(errorKey)}
          </p>
        )}
        {status.restartRequired ? (
          <Button type="button" onClick={onRestart} disabled={recovery.submitting}>
            {t('profileRecovery.restart')}
          </Button>
        ) : (
          <Button type="submit" disabled={!recovery.canSubmit}>
            {t(recovery.busy ? 'profileRecovery.recovering' : 'profileRecovery.submit')}
          </Button>
        )}
        <p id="recovery-help" className="text-ui-caption text-muted-foreground">
          {t('profileRecovery.help')}
        </p>
      </form>
    </AppStateShell>
  )
}

function ProfileRecoverySupportView({ status }: { status: ProfileRecoveryResponse }) {
  const { t } = useTranslation()
  const { exportState, exportDiagnostics } = useDiagnosticsExport()
  const admission = status.admission
  const admissionRequired = status.state === 'admission_recovery_required'

  return (
    <AppStateShell
      title={t(admissionRequired ? 'profileAdmissionRecovery.title' : 'profileRecovery.title')}
      description={t(
        admissionRequired ? 'profileAdmissionRecovery.description' : 'profileRecovery.partial'
      )}
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
