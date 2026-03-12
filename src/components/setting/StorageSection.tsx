import { Database, FolderOpen, HardDrive, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import ClearHistoryDialog from './ClearHistoryDialog'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import {
  Button,
  Switch,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { Skeleton } from '@/components/ui/skeleton'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useSetting } from '@/hooks/useSetting'
import { invokeWithTrace } from '@/lib/tauri-command'
import type { RetentionRule } from '@/types/setting'

// ── Types ────────────────────────────────────────────────────────────

interface StorageStats {
  databaseBytes: number
  vaultBytes: number
  cacheBytes: number
  logsBytes: number
  totalBytes: number
  dataDir: string
}

// ── Constants ────────────────────────────────────────────────────────

const SECONDS_PER_DAY = 86400

/**
 * Storage category definitions — each segment in the usage bar.
 * Colors use the app's oklch chart palette for cohesion with the theme system.
 */
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

const RETENTION_DAYS_OPTIONS = [
  { value: '7', days: 7 },
  { value: '14', days: 14 },
  { value: '30', days: 30 },
  { value: '60', days: 60 },
  { value: '90', days: 90 },
  { value: '180', days: 180 },
  { value: '365', days: 365 },
] as const

const MAX_ITEMS_OPTIONS = [
  { value: '100', count: 100 },
  { value: '200', count: 200 },
  { value: '500', count: 500 },
  { value: '1000', count: 1000 },
  { value: '2000', count: 2000 },
  { value: '5000', count: 5000 },
] as const

// ── Helpers ──────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const k = 1024
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1)
  const value = bytes / Math.pow(k, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

function getByAgeSecs(rules: RetentionRule[]): number | null {
  for (const rule of rules) {
    if ('by_age' in rule) return rule.by_age.max_age
  }
  return null
}

function getByCountItems(rules: RetentionRule[]): number | null {
  for (const rule of rules) {
    if ('by_count' in rule) return rule.by_count.max_items
  }
  return null
}

function setByAgeRule(rules: RetentionRule[], days: number): RetentionRule[] {
  const newRule: RetentionRule = { by_age: { max_age: days * SECONDS_PER_DAY } }
  return [newRule, ...rules.filter(r => !('by_age' in r))]
}

function setByCountRule(rules: RetentionRule[], maxItems: number): RetentionRule[] {
  const newRule: RetentionRule = { by_count: { max_items: maxItems } }
  return [...rules.filter(r => !('by_count' in r)), newRule]
}

// ── StorageBar sub-component ─────────────────────────────────────────

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
 *
 * 骨架屏占位，与真实用量条的布局完全一致，避免加载时的视觉跳动。
 */
function StorageUsageSkeleton() {
  return (
    <div className="px-4 py-4 space-y-3.5">
      {/* Header skeleton */}
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-2">
          <Skeleton className="h-6 w-20" />
          <Skeleton className="h-3 w-8" />
        </div>
        <Skeleton className="h-6 w-6 rounded-md" />
      </div>

      {/* Bar skeleton */}
      <Skeleton className="h-3 w-full rounded-full" />

      {/* Legend skeleton — 2×2 grid matching real layout */}
      <div className="grid grid-cols-2 gap-x-6 gap-y-1.5">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="flex items-center gap-2 min-w-0">
            <Skeleton className="w-2 h-2 rounded-full shrink-0" />
            <Skeleton className="w-3 h-3 rounded shrink-0" />
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
      <div className="px-4 py-6 flex flex-col items-center justify-center gap-3 text-center">
        <div className="text-sm text-destructive">{error}</div>
        <Button variant="outline" size="sm" onClick={onRefresh}>
          <RefreshCw className="w-4 h-4 mr-2" />
          {t('common.retry')}
        </Button>
      </div>
    )
  }

  return (
    <div className="px-4 py-4 space-y-3.5">
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
          onClick={onRefresh}
          className="p-1.5 rounded-md text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/60 transition-colors"
          aria-label="Refresh"
        >
          <RefreshCw className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Segmented bar */}
      <TooltipProvider>
        <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted/50 gap-px">
          {segments.map(seg =>
            seg.percentage > 0 ? (
              <Tooltip key={seg.key}>
                <TooltipTrigger asChild>
                  <div
                    className="h-full transition-all duration-500 ease-out first:rounded-l-full last:rounded-r-full cursor-default"
                    style={{
                      width: `${Math.max(seg.percentage, 2)}%`,
                      backgroundColor: seg.color,
                      opacity: 0.85,
                    }}
                  />
                </TooltipTrigger>
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
                className="w-2 h-2 rounded-full shrink-0"
                style={{ backgroundColor: seg.color, opacity: 0.85 }}
              />
              <Icon className="w-3 h-3 text-muted-foreground/50 shrink-0" />
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

// ── Main Component ───────────────────────────────────────────────────

const StorageSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateRetentionPolicy } = useSetting()

  // Retention policy state
  const [enabled, setEnabled] = useState(true)
  const [retentionDays, setRetentionDays] = useState('30')
  const [maxItems, setMaxItems] = useState('500')
  const [skipPinned, setSkipPinned] = useState(true)

  // Storage stats state
  const [stats, setStats] = useState<StorageStats | null>(null)
  const [statsLoading, setStatsLoading] = useState(true)
  const [statsError, setStatsError] = useState<string | null>(null)

  // Action states
  const [clearingCache, setClearingCache] = useState(false)
  const [clearingHistory, setClearingHistory] = useState(false)
  const [showClearHistoryDialog, setShowClearHistoryDialog] = useState(false)

  // ── Load storage stats ───────────────────────────────────────────

  const loadStats = useCallback(async () => {
    setStatsLoading(true)
    setStatsError(null)
    try {
      const result = await invokeWithTrace<StorageStats>('get_storage_stats')
      setStats(result)
    } catch (err) {
      console.error('Failed to load storage stats:', err)
      setStatsError(err instanceof Error ? err.message : String(err))
    } finally {
      setStatsLoading(false)
    }
  }, [])

  useEffect(() => {
    void loadStats()
  }, [loadStats])

  // ── Compute bar segments ─────────────────────────────────────────

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

  // ── Sync retention policy from backend ───────────────────────────

  useEffect(() => {
    if (!setting?.retention_policy) return
    const rp = setting.retention_policy

    setEnabled(rp.enabled)
    setSkipPinned(rp.skip_pinned)

    const ageSecs = getByAgeSecs(rp.rules)
    if (ageSecs !== null) {
      const days = Math.round(ageSecs / SECONDS_PER_DAY)
      const match = RETENTION_DAYS_OPTIONS.find(o => o.days === days)
      setRetentionDays(match ? match.value : '30')
    }

    const count = getByCountItems(rp.rules)
    if (count !== null) {
      const match = MAX_ITEMS_OPTIONS.find(o => o.count === count)
      setMaxItems(match ? match.value : '500')
    }
  }, [setting?.retention_policy])

  // ── Handlers ─────────────────────────────────────────────────────

  const handleEnabledChange = async (checked: boolean) => {
    const prev = enabled
    setEnabled(checked)
    try {
      await updateRetentionPolicy({ enabled: checked })
    } catch (err) {
      console.error('Failed to update retention enabled:', err)
      setEnabled(prev)
    }
  }

  const handleRetentionDaysChange = async (value: string) => {
    const prev = retentionDays
    setRetentionDays(value)
    if (!setting?.retention_policy) return
    const days = RETENTION_DAYS_OPTIONS.find(o => o.value === value)?.days ?? 30
    try {
      await updateRetentionPolicy({ rules: setByAgeRule(setting.retention_policy.rules, days) })
    } catch (err) {
      console.error('Failed to update retention days:', err)
      setRetentionDays(prev)
    }
  }

  const handleMaxItemsChange = async (value: string) => {
    const prev = maxItems
    setMaxItems(value)
    if (!setting?.retention_policy) return
    const count = MAX_ITEMS_OPTIONS.find(o => o.value === value)?.count ?? 500
    try {
      await updateRetentionPolicy({ rules: setByCountRule(setting.retention_policy.rules, count) })
    } catch (err) {
      console.error('Failed to update max items:', err)
      setMaxItems(prev)
    }
  }

  const handleSkipPinnedChange = async (checked: boolean) => {
    const prev = skipPinned
    setSkipPinned(checked)
    try {
      await updateRetentionPolicy({ skip_pinned: checked })
    } catch (err) {
      console.error('Failed to update skip pinned:', err)
      setSkipPinned(prev)
    }
  }

  const handleClearCache = async () => {
    setClearingCache(true)
    try {
      await invokeWithTrace('clear_cache')
      await loadStats()
    } catch (err) {
      console.error('Failed to clear cache:', err)
    } finally {
      setClearingCache(false)
    }
  }

  const handleClearHistory = async () => {
    setClearingHistory(true)
    try {
      await invokeWithTrace('clear_all_clipboard_history')
      await loadStats()
    } catch (err) {
      console.error('Failed to clear history:', err)
      throw err
    } finally {
      setClearingHistory(false)
    }
  }

  const handleOpenDataDir = async () => {
    try {
      await invokeWithTrace('open_data_directory')
    } catch (err) {
      console.error('Failed to open data directory:', err)
    }
  }

  if (error) {
    return (
      <div className="text-destructive py-4">
        {t('settings.sections.storage.loadError')} {error}
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* ── Storage Usage ── */}
      <SettingGroup title={t('settings.sections.storage.storageUsage.label')}>
        <StorageUsageBar
          segments={segments}
          total={stats?.totalBytes ?? 0}
          loading={statsLoading}
          error={statsError}
          onRefresh={loadStats}
        />
      </SettingGroup>

      {/* ── Retention Policy ── */}
      <SettingGroup title={t('settings.categories.storage')}>
        <SettingRow
          label={t('settings.sections.storage.autoClearHistory.label')}
          description={t('settings.sections.storage.autoClearHistory.description')}
        >
          <Switch checked={enabled} onCheckedChange={handleEnabledChange} />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.historyRetention.label')}
          description={t('settings.sections.storage.historyRetention.description')}
        >
          <Select
            value={retentionDays}
            onValueChange={handleRetentionDaysChange}
            disabled={!enabled}
          >
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {RETENTION_DAYS_OPTIONS.map(opt => (
                <SelectItem key={opt.value} value={opt.value}>
                  {t('settings.sections.storage.historyRetention.days', { days: opt.days })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.maxHistoryItems.label')}
          description={t('settings.sections.storage.maxHistoryItems.description')}
        >
          <Select value={maxItems} onValueChange={handleMaxItemsChange} disabled={!enabled}>
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {MAX_ITEMS_OPTIONS.map(opt => (
                <SelectItem key={opt.value} value={opt.value}>
                  {t('settings.sections.storage.maxHistoryItems.items', { count: opt.count })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.skipPinned.label')}
          description={t('settings.sections.storage.skipPinned.description')}
        >
          <Switch
            checked={skipPinned}
            onCheckedChange={handleSkipPinnedChange}
            disabled={!enabled}
          />
        </SettingRow>
      </SettingGroup>

      {/* ── Data Management ── */}
      <SettingGroup title={t('settings.sections.storage.dataDirectory.label')}>
        <SettingRow
          label={t('settings.sections.storage.clearCache.label')}
          description={t('settings.sections.storage.clearCache.description')}
        >
          <Button variant="outline" size="sm" onClick={handleClearCache} disabled={clearingCache}>
            {clearingCache
              ? t('settings.sections.storage.clearCache.clearing')
              : t('settings.sections.storage.clearCache.button')}
          </Button>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.clearHistory.label')}
          description={t('settings.sections.storage.clearHistory.description')}
        >
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setShowClearHistoryDialog(true)}
            disabled={clearingHistory}
          >
            {clearingHistory
              ? t('settings.sections.storage.clearHistory.clearing')
              : t('settings.sections.storage.clearHistory.button')}
          </Button>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.dataDirectory.label')}
          description={t('settings.sections.storage.dataDirectory.description')}
        >
          <Button variant="outline" size="sm" onClick={handleOpenDataDir}>
            {t('settings.sections.storage.dataDirectory.button')}
          </Button>
        </SettingRow>
      </SettingGroup>

      {/* ── Confirmation Dialog ── */}
      <ClearHistoryDialog
        open={showClearHistoryDialog}
        onOpenChange={setShowClearHistoryDialog}
        onConfirm={handleClearHistory}
      />
    </div>
  )
}

export default StorageSection
