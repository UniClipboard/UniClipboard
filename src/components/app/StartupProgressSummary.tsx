import { useTranslation } from 'react-i18next'
import type { StepProgress } from '@/lib/startup-progress'

type Props = {
  current: StepProgress | undefined
  elapsed: number
  finishingStep: boolean | undefined
  percentage: number | null
}

export function StartupProgressSummary({ current, elapsed, finishingStep, percentage }: Props) {
  const { t, i18n } = useTranslation()
  const formatNumber = (value: number) => value.toLocaleString(i18n.language)
  return (
    <section className="mt-8" aria-label={t('upgradeProgress.progress')}>
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2 text-ui-body">
        <span className="font-medium">
          {t(`upgradeProgress.steps.${current?.step ?? 'preparing'}`)}
        </span>
        <span className="tabular-nums text-muted-foreground">
          {percentage === null
            ? t(finishingStep ? 'upgradeProgress.finishingStep' : 'upgradeProgress.processing')
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
  )
}
