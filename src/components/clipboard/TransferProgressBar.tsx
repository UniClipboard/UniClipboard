import { ArrowDownToLine, ArrowUpFromLine } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { Progress } from '@/components/ui/progress'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatDuration, formatFileSize } from '@/utils'

interface TransferProgressBarProps {
  progress: TransferProgressInfo
  variant?: 'compact' | 'detailed'
}

const TransferProgressBar: React.FC<TransferProgressBarProps> = ({
  progress,
  variant = 'compact',
}) => {
  const { t } = useTranslation()

  const percent =
    progress.totalBytes && progress.totalBytes > 0
      ? Math.round((progress.bytesTransferred / progress.totalBytes) * 100)
      : progress.totalChunks > 0
        ? Math.round((progress.chunksCompleted / progress.totalChunks) * 100)
        : 0
  const speedLabel = progress.bytesPerSecond
    ? t('clipboard.transfer.speedValue', {
        speed: `${formatFileSize(progress.bytesPerSecond)}/s`,
      })
    : null
  const remainingLabel =
    progress.estimatedRemainingSeconds !== null
      ? t('clipboard.transfer.remainingValue', {
          time: formatDuration(progress.estimatedRemainingSeconds),
        })
      : null

  const DirectionIcon = progress.direction === 'Sending' ? ArrowUpFromLine : ArrowDownToLine
  const directionLabel =
    progress.direction === 'Sending'
      ? t('clipboard.transfer.sending')
      : t('clipboard.transfer.receiving')

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

  // Detailed variant for preview panel
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <DirectionIcon className="h-4 w-4 text-primary" />
        <span className="text-sm font-medium">{directionLabel}</span>
        <span className="text-sm text-muted-foreground ml-auto">{percent}%</span>
      </div>
      <Progress value={percent} className="h-2" />
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>
          {t('clipboard.transfer.progress', {
            transferred: formatFileSize(progress.bytesTransferred),
            total: progress.totalBytes ? formatFileSize(progress.totalBytes) : '?',
            percent,
          })}
        </span>
        <span>
          {t('clipboard.transfer.chunks', {
            completed: progress.chunksCompleted,
            total: progress.totalChunks,
          })}
        </span>
      </div>
      {(speedLabel || remainingLabel) && (
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{speedLabel ?? t('clipboard.transfer.speedPending')}</span>
          <span>{remainingLabel ?? t('clipboard.transfer.remainingPending')}</span>
        </div>
      )}
    </div>
  )
}

export default TransferProgressBar
