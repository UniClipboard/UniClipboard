import { ArrowDownToLine, ArrowUpFromLine, Info } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Progress } from '@/components/ui/progress'
import { cn } from '@/lib/utils'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatDuration, formatFileSize } from '@/utils'

interface TransferProgressBarProps {
  progress: TransferProgressInfo
  variant?: 'compact' | 'inline' | 'minimal'
}

const TransferProgressBar: React.FC<TransferProgressBarProps> = ({
  progress,
  variant = 'inline',
}) => {
  const { t } = useTranslation()

  const percent =
    progress.totalBytes && progress.totalBytes > 0
      ? Math.round((progress.bytesTransferred / progress.totalBytes) * 100)
      : progress.totalChunks > 0
        ? Math.round((progress.chunksCompleted / progress.totalChunks) * 100)
        : 0
  const speedLabel = progress.bytesPerSecond ? formatFileSize(progress.bytesPerSecond) + '/s' : null
  const remainingLabel =
    progress.estimatedRemainingSeconds !== null
      ? formatDuration(progress.estimatedRemainingSeconds)
      : null

  const DirectionIcon = progress.direction === 'Sending' ? ArrowUpFromLine : ArrowDownToLine
  const directionLabel =
    progress.direction === 'Sending'
      ? t('clipboard.transfer.sending')
      : t('clipboard.transfer.receiving')

  if (variant === 'minimal') {
    return (
      <div className="flex items-center gap-2 text-[10px] font-medium tabular-nums text-primary/80">
        <span>{percent}%</span>
        {speedLabel && (
          <>
            <span className="opacity-30">•</span>
            <span>{speedLabel}</span>
          </>
        )}
        {remainingLabel && (
          <>
            <span className="opacity-30">•</span>
            <span>{remainingLabel}</span>
          </>
        )}
      </div>
    )
  }

  if (variant === 'compact') {
    return (
      <div className="flex items-center gap-1.5 w-full">
        <DirectionIcon className="h-3 w-3 shrink-0 text-primary" />
        <Progress value={percent} className="h-1.5 flex-1" />
        <span className="text-xs text-muted-foreground shrink-0">{percent}%</span>
        {speedLabel && (
          <span className="text-[11px] text-muted-foreground shrink-0">{speedLabel}</span>
        )}
      </div>
    )
  }

  return (
    <div className="flex items-center gap-2 rounded-lg border border-primary/15 bg-primary/6 px-2.5 py-2">
      <DirectionIcon className="h-3.5 w-3.5 shrink-0 text-primary" />
      <div className="min-w-0 flex-1">
        <div className="mb-1.5 flex items-center justify-between gap-2">
          <span className="truncate text-[11px] font-medium text-foreground/85">
            {directionLabel}
          </span>
          <span className="shrink-0 text-[11px] text-muted-foreground">{percent}%</span>
        </div>
        <Progress value={percent} className="h-1.5 bg-primary/10" />
      </div>
      <Popover>
        <PopoverTrigger asChild>
          <button
            type="button"
            className={cn(
              'flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-primary/15 bg-background/80 text-muted-foreground transition-colors',
              'hover:border-primary/30 hover:text-primary'
            )}
            aria-label={t('clipboard.preview.information')}
          >
            <Info className="h-3.5 w-3.5" />
          </button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-72">
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              <DirectionIcon className="h-4 w-4 text-primary" />
              <span className="text-sm font-medium">{directionLabel}</span>
              <span className="ml-auto text-xs text-muted-foreground">{percent}%</span>
            </div>
            <div className="rounded-lg bg-muted/35 p-3">
              <div className="space-y-2 text-xs text-muted-foreground">
                <div className="flex items-center justify-between gap-3">
                  <span>
                    {t('clipboard.transfer.progress', {
                      transferred: formatFileSize(progress.bytesTransferred),
                      total: progress.totalBytes ? formatFileSize(progress.totalBytes) : '?',
                      percent,
                    })}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>
                    {t('clipboard.transfer.chunks', {
                      completed: progress.chunksCompleted,
                      total: progress.totalChunks,
                    })}
                  </span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>{speedLabel ?? t('clipboard.transfer.speedPending')}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>{remainingLabel ?? t('clipboard.transfer.remainingPending')}</span>
                </div>
              </div>
            </div>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}

export default TransferProgressBar
