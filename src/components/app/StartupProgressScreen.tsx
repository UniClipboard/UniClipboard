import {
  AlertCircle,
  ArrowUpCircle,
  Check,
  ChevronDown,
  Download,
  Loader2,
  MessageCircle,
  RotateCw,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { contactAuthor, STARTUP_SUPPORT_URL } from '@/api/startup-support'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { Button } from '@/components/ui/button'
import { useStartupElapsed } from '@/hooks/useStartupElapsed'
import { startupPresentation, stepPercentage, type StartupSnapshot } from '@/lib/startup-progress'
import appIcon from '@/updater/app-icon.png'

interface Props {
  snapshot: StartupSnapshot
  onRetry: () => void
  onExport: () => Promise<void | boolean> | void
}

export function StartupProgressScreen({ snapshot, onRetry, onExport }: Props) {
  const { t, i18n } = useTranslation()
  const [exportState, setExportState] = useState<'idle' | 'working' | 'failed' | 'done'>('idle')
  const [contactState, setContactState] = useState<'idle' | 'working' | 'failed' | 'done'>('idle')
  const [updateState, setUpdateState] = useState<'idle' | 'working' | 'failed'>('idle')
  const current = snapshot.upgrade?.steps.find(step => step.step === snapshot.upgrade?.current_step)
  const percentage = stepPercentage(current)
  const { required, failed, ready, title, showProgress, showActivity } =
    startupPresentation(snapshot)
  const formatNumber = (value: number) => value.toLocaleString(i18n.language)
  const elapsed = useStartupElapsed(snapshot.attempt_id, snapshot.elapsed_ms, !failed && !ready)
  const finishingStep =
    current && current.unit !== null && current.total !== null && current.processed >= current.total

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
    <main className="flex min-h-0 flex-1 overflow-y-auto bg-background text-foreground">
      <div className="m-auto w-full max-w-2xl px-6 py-12 sm:px-12">
        <div className="mb-10 flex items-center gap-3">
          <img src={appIcon} alt="" className="size-10 shrink-0" />
          <span className="text-ui-section font-semibold">UniClipboard</span>
        </div>
        <div className="mb-4 flex items-center gap-2 text-ui-body text-muted-foreground">
          {failed ? (
            <AlertCircle className="size-4 text-destructive" />
          ) : ready ? (
            <Check className="size-4 text-emerald-600" />
          ) : (
            <Loader2 className="size-4 animate-spin motion-reduce:animate-none" />
          )}
          {t(required ? 'upgradeProgress.category' : 'upgradeProgress.startupCategory')}
        </div>
        <h1 className="text-ui-title font-semibold" aria-live="polite">
          {t(`upgradeProgress.${title}`)}
        </h1>
        <p className="mt-3 text-ui-body text-muted-foreground">
          {failed
            ? t(`upgradeProgress.errors.${snapshot.failure?.reason ?? 'interrupted'}`)
            : t(
                `upgradeProgress.${ready && required ? 'readyDescription' : required && snapshot.state === 'upgrading' ? 'description' : 'startingDescription'}`
              )}
        </p>

        {showProgress && (
          <section className="mt-8" aria-label={t('upgradeProgress.progress')}>
            <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2 text-ui-body">
              <span className="font-medium">
                {t(`upgradeProgress.steps.${current?.step ?? 'preparing'}`)}
              </span>
              <span className="tabular-nums text-muted-foreground">
                {percentage === null
                  ? t(
                      finishingStep ? 'upgradeProgress.finishingStep' : 'upgradeProgress.processing'
                    )
                  : t('upgradeProgress.stepPercent', { percent: percentage })}
              </span>
            </div>
            <progress
              aria-label={t('upgradeProgress.progress')}
              max={100}
              value={percentage ?? undefined}
              className="sr-only"
            />
            <div aria-hidden="true" className="h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className={`h-full rounded-full bg-primary transition-[width] duration-300 motion-reduce:transition-none ${percentage === null ? 'w-1/3 animate-pulse motion-reduce:animate-none' : ''}`}
                style={percentage === null ? undefined : { width: `${percentage}%` }}
              />
            </div>
            <div className="mt-3 flex flex-wrap justify-between gap-2 text-ui-caption text-muted-foreground">
              <span>
                {current?.unit
                  ? t('upgradeProgress.count', {
                      processed: formatNumber(current.processed),
                      total:
                        current.total === null
                          ? t('upgradeProgress.unknown')
                          : formatNumber(current.total),
                      unit: t(`upgradeProgress.units.${current.unit}`),
                    })
                  : t('upgradeProgress.processing')}
              </span>
              <span className="tabular-nums">
                {t('upgradeProgress.elapsed', {
                  time: `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, '0')}`,
                })}
              </span>
            </div>
          </section>
        )}

        {!required && !failed && !ready && (
          <p className="mt-5 text-ui-caption tabular-nums text-muted-foreground">
            {t('upgradeProgress.elapsed', {
              time: `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, '0')}`,
            })}
          </p>
        )}
        {showActivity && (
          <details className="group mt-8 border-t border-border pt-4">
            <summary className="flex cursor-pointer list-none items-center justify-between text-ui-body text-muted-foreground hover:text-foreground [&::-webkit-details-marker]:hidden">
              {t('upgradeProgress.activity')}
              <ChevronDown className="size-4 transition-transform group-open:rotate-180" />
            </summary>
            <ol className="mt-4 flex flex-col gap-4 text-ui-body">
              {snapshot.upgrade?.steps.map(step => (
                <li key={step.step} className="flex items-start gap-3">
                  {step.completed ? (
                    <Check className="mt-0.5 size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
                  ) : failed ? (
                    <AlertCircle className="mt-0.5 size-4 shrink-0 text-destructive" />
                  ) : (
                    <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap justify-between gap-2">
                      <span>{t(`upgradeProgress.steps.${step.step}`)}</span>
                      <span className="text-ui-caption text-muted-foreground">
                        {t(
                          `upgradeProgress.${step.completed ? 'stepDone' : failed ? 'stepStopped' : 'processing'}`
                        )}
                      </span>
                    </div>
                    {step.warning_count === null ? (
                      <p className="mt-1 text-ui-caption text-muted-foreground">
                        {t('upgradeProgress.warningsUnknown')}
                      </p>
                    ) : (
                      step.warning_count > 0 && (
                        <p className="mt-1 text-ui-caption text-amber-700 dark:text-amber-400">
                          {t('upgradeProgress.warnings', { count: step.warning_count })}
                        </p>
                      )
                    )}
                  </div>
                </li>
              ))}
            </ol>
          </details>
        )}
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
      </div>
    </main>
  )
}
