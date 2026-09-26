import { AlertCircle, Check, ChevronDown, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { StartupSnapshot } from '@/lib/startup-progress'

type Props = { failed: boolean; snapshot: StartupSnapshot }

export function StartupActivity({ failed, snapshot }: Props) {
  const { t } = useTranslation()
  return (
    <details className="group mt-8 border-t border-border pt-3">
      <summary className="flex cursor-pointer list-none items-center justify-between text-ui-body text-muted-foreground hover:text-foreground [&::-webkit-details-marker]:hidden">
        {t('upgradeProgress.activity')}
        <ChevronDown
          className="size-4 transition-transform group-open:rotate-180 motion-reduce:transition-none"
          aria-hidden="true"
        />
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
  )
}
