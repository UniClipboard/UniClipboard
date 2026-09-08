import { useTranslation } from 'react-i18next'
import { Progress } from '@/components/ui/progress'
import { cn } from '@/lib/utils'
import { ReleaseNotes } from './ReleaseNotes'

interface UpdateDetailsProps {
  currentVersion?: string
  version?: string
  body?: string | null
  phase: string
  percent: number | null
  showReadyHint?: boolean
}

export function UpdateDetails({
  currentVersion,
  version,
  body,
  phase,
  percent,
  showReadyHint = false,
}: UpdateDetailsProps) {
  const { t } = useTranslation()
  const progressing = phase === 'downloading' || phase === 'installing'
  return (
    <>
      <div className="space-y-1 text-sm">
        <div className="flex items-center justify-between text-muted-foreground">
          <span>{t('update.currentVersion')}</span>
          <span className="text-foreground">{currentVersion ?? '-'}</span>
        </div>
        <div className="flex items-center justify-between text-muted-foreground">
          <span>{t('update.latestVersion')}</span>
          <span className="text-foreground">{version ?? '-'}</span>
        </div>
      </div>
      <div className="space-y-2">
        <div className="text-sm font-medium text-foreground">{t('update.releaseNotes')}</div>
        <div className="max-h-48 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
          <ReleaseNotes content={body ?? ''} fallback={t('update.noNotes')} />
        </div>
      </div>
      {showReadyHint && (
        <div className="text-xs text-emerald-600 dark:text-emerald-400 pt-1">
          {t('update.readyHint')}
        </div>
      )}
      {progressing && (
        <div className="space-y-2 pt-2">
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{phase === 'installing' ? t('update.installing') : t('update.downloading')}</span>
            {percent !== null && <span>{Math.round(percent)}%</span>}
          </div>
          <Progress
            value={percent ?? undefined}
            className={cn('h-2', percent === null && 'animate-pulse')}
          />
        </div>
      )}
    </>
  )
}
