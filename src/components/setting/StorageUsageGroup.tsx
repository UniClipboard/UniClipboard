import { Database, FolderOpen, HardDrive, RefreshCw } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type { StorageStats } from '@/api/storage'
import { Button } from '@/components/ui'
import { Skeleton } from '@/components/ui/skeleton'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { SettingGroup } from './SettingGroup'
const STORAGE_CATEGORIES = [
  {
    key: 'database' as const,
    color: 'var(--chart-1)',
    icon: Database,
  },
  {
    key: 'vault' as const,
    color: 'var(--chart-2)',
    icon: HardDrive,
  },
  {
    key: 'cache' as const,
    color: 'var(--chart-3)',
    icon: FolderOpen,
  },
  {
    key: 'logs' as const,
    color: 'var(--chart-4)',
    icon: FolderOpen,
  },
] as const

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const k = 1024
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1)
  const value = bytes / Math.pow(k, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

interface StorageSegment {
  key: string
  label: string
  bytes: number
  percentage: number
  color: string
  icon: React.ComponentType<{ className?: string }>
}

/**
 * Skeleton placeholder that mirrors the exact layout of the real usage bar.
 */
function StorageUsageSkeleton() {
  return (
    <div className="px-1 py-4 space-y-3.5">
      {/* Header skeleton */}
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-2">
          <Skeleton className="h-6 w-20" />
          <Skeleton className="h-3 w-8" />
        </div>
        <Skeleton className="size-6 rounded-md" />
      </div>

      {/* Bar skeleton */}
      <Skeleton className="h-3 w-full rounded-full" />

      {/* Legend skeleton — 2×2 grid matching real layout */}
      <div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="flex items-center gap-2 min-w-0">
            <Skeleton className="size-2 rounded-full shrink-0" />
            <Skeleton className="size-3 rounded shrink-0" />
            <Skeleton className="h-3 w-12" />
            <Skeleton className="h-3 w-10 ml-auto" />
          </div>
        ))}
      </div>
    </div>
  )
}

function StorageUsageBar({
  segments,
  total,
  loading,
  error,
  onRefresh,
}: {
  segments: StorageSegment[]
  total: number
  loading: boolean
  error?: string | null
  onRefresh: () => void
}) {
  const { t } = useTranslation()

  if (loading) return <StorageUsageSkeleton />

  if (error) {
    return (
      <div className="px-1 py-6 flex flex-col items-center justify-center gap-3 text-center">
        <div className="text-sm text-destructive">{error}</div>
        <Button variant="outline" size="sm" onClick={onRefresh}>
          <RefreshCw className="size-4 mr-2" />
          {t('common.retry')}
        </Button>
      </div>
    )
  }

  return (
    <div className="px-1 py-4 space-y-3.5">
      {/* Header: total + refresh */}
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-2">
          <span className="text-xl font-semibold tabular-nums tracking-tight">
            {formatBytes(total)}
          </span>
          <span className="text-xs text-muted-foreground">
            {t('settings.sections.storage.storageUsage.total')}
          </span>
        </div>
        <button
          type="button"
          onClick={onRefresh}
          className="p-1.5 rounded-md text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/60 transition-colors"
          aria-label="Refresh"
        >
          <RefreshCw className="size-3.5" />
        </button>
      </div>

      {/* Segmented bar */}
      <TooltipProvider>
        <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted/50 gap-px">
          {segments.map(seg =>
            seg.percentage > 0 ? (
              <Tooltip key={seg.key}>
                <TooltipTrigger
                  render={
                    <div
                      className="h-full transition-[width] duration-500 ease-out first:rounded-l-full last:rounded-r-full cursor-default"
                      style={{
                        width: `${Math.max(seg.percentage, 2)}%`,
                        backgroundColor: seg.color,
                        opacity: 0.85,
                      }}
                    />
                  }
                />
                <TooltipContent>
                  <span className="font-medium">{seg.label}</span>
                  <span className="ml-1.5 opacity-70">{formatBytes(seg.bytes)}</span>
                </TooltipContent>
              </Tooltip>
            ) : null
          )}
        </div>
      </TooltipProvider>

      {/* Legend grid */}
      <div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
        {segments.map(seg => {
          const Icon = seg.icon
          return (
            <div key={seg.key} className="flex items-center gap-2 min-w-0">
              <span
                className="size-2 rounded-full shrink-0"
                style={{ backgroundColor: seg.color, opacity: 0.85 }}
              />
              <Icon className="size-3 text-muted-foreground/50 shrink-0" />
              <span className="text-xs text-muted-foreground truncate">{seg.label}</span>
              <span className="text-xs tabular-nums text-foreground/70 ml-auto shrink-0">
                {formatBytes(seg.bytes)}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function StorageUsageGroup({
  stats,
  loading,
  error,
  onRefresh,
}: {
  stats: StorageStats | null
  loading: boolean
  error: string | null
  onRefresh: () => Promise<void>
}) {
  const { t } = useTranslation()
  const segments = useMemo<StorageSegment[]>(() => {
    if (!stats) return []
    const total = stats.totalBytes || 1 // avoid division by zero
    const bytesMap: Record<string, number> = {
      database: stats.databaseBytes,
      vault: stats.vaultBytes,
      cache: stats.cacheBytes,
      logs: stats.logsBytes,
    }
    const labelMap: Record<string, string> = {
      database: t('settings.sections.storage.storageUsage.database'),
      vault: t('settings.sections.storage.storageUsage.blobVault'),
      cache: t('settings.sections.storage.storageUsage.cache'),
      logs: t('settings.sections.storage.storageUsage.logs'),
    }
    return STORAGE_CATEGORIES.map(cat => ({
      key: cat.key,
      label: labelMap[cat.key],
      bytes: bytesMap[cat.key],
      percentage: (bytesMap[cat.key] / total) * 100,
      color: cat.color,
      icon: cat.icon,
    }))
  }, [stats, t])

  return (
    <SettingGroup title={t('settings.sections.storage.storageUsage.label')}>
      <StorageUsageBar
        segments={segments}
        total={stats?.totalBytes ?? 0}
        loading={loading}
        error={error}
        onRefresh={() => {
          void onRefresh()
        }}
      />
    </SettingGroup>
  )
}
