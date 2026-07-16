import { Database, FolderOpen, HardDrive, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getSearchStatus, triggerSearchRebuild } from '@/api/daemon'
import type { SearchStatusData } from '@/api/daemon'
import * as storageApi from '@/api/storage'
import type { StorageStats } from '@/api/storage'
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
import { createLogger } from '@/lib/logger'
import type { RetentionPolicy, RetentionRule } from '@/types/setting'
import ClearHistoryDialog from './ClearHistoryDialog'
import { ConfigBackupGroup } from './ConfigBackupGroup'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'

const log = createLogger('storage-section')

// ── Constants ────────────────────────────────────────────────────────

const SECONDS_PER_DAY = 86400
const DEFAULT_RETENTION_POLICY: RetentionPolicy = {
  enabled: true,
  rules: [{ byAge: { maxAge: 30 * SECONDS_PER_DAY } }],
  skipPinned: true,
  evaluation: 'anyMatch',
}

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
  { value: '100', count: 100 as number | null },
  { value: '200', count: 200 as number | null },
  { value: '500', count: 500 as number | null },
  { value: '1000', count: 1000 as number | null },
  { value: '2000', count: 2000 as number | null },
  { value: '5000', count: 5000 as number | null },
  { value: 'unlimited', count: null as number | null },
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
    if ('byAge' in rule) return rule.byAge.maxAge
  }
  return null
}

function getByCountItems(rules: RetentionRule[]): number | null {
  for (const rule of rules) {
    if ('byCount' in rule) return rule.byCount.maxItems
  }
  return null
}

function setByAgeRule(rules: RetentionRule[], days: number): RetentionRule[] {
  const newRule: RetentionRule = { byAge: { maxAge: days * SECONDS_PER_DAY } }
  return [newRule, ...rules.filter(r => !('byAge' in r))]
}

function setByCountRule(rules: RetentionRule[], maxItems: number): RetentionRule[] {
  const newRule: RetentionRule = { byCount: { maxItems: maxItems } }
  return [...rules.filter(r => !('byCount' in r)), newRule]
}

/** Drop the `byCount` rule entirely — absence means "no count cap". */
function clearByCountRule(rules: RetentionRule[]): RetentionRule[] {
  return rules.filter(r => !('byCount' in r))
}

/**
 * Resolve the persisted `byCount` value for a `MAX_ITEMS_OPTIONS` select
 * value; `null` means "no count cap" (the "unlimited" option).
 *
 * Falls back to 500 only if `value` doesn't match any known option (shouldn't
 * happen — the Select only offers known values). Note this must NOT be
 * written as `MAX_ITEMS_OPTIONS.find(...)?.count ?? 500`: when the option is
 * found and its `count` is legitimately `null`, `??` treats `null` as
 * nullish and incorrectly falls through to `500`.
 */
export function resolveMaxItemsCount(value: string): number | null {
  const opt = MAX_ITEMS_OPTIONS.find(o => o.value === value)
  return opt ? opt.count : 500
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
    <div className="p-4 space-y-3.5">
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
      <div className="px-4 py-6 flex flex-col items-center justify-center gap-3 text-center">
        <div className="text-sm text-destructive">{error}</div>
        <Button variant="outline" size="sm" onClick={onRefresh}>
          <RefreshCw className="size-4 mr-2" />
          {t('common.retry')}
        </Button>
      </div>
    )
  }

  return (
    <div className="p-4 space-y-3.5">
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

// ── Main Component ───────────────────────────────────────────────────

const StorageSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateRetentionPolicy } = useSetting()

  const [retentionPolicy, setRetentionPolicy] = useOptimisticSetting<RetentionPolicy>(
    setting?.retentionPolicy ?? DEFAULT_RETENTION_POLICY,
    next => updateRetentionPolicy(next),
    { failureLog: 'Failed to update retention policy' }
  )
  const enabled = retentionPolicy.enabled
  const skipPinned = retentionPolicy.skipPinned
  const ageSecs = getByAgeSecs(retentionPolicy.rules)
  const retentionDays =
    RETENTION_DAYS_OPTIONS.find(
      option => option.days === Math.round((ageSecs ?? 0) / SECONDS_PER_DAY)
    )?.value ?? '30'
  const countItems = getByCountItems(retentionPolicy.rules)
  const maxItems =
    countItems === null
      ? 'unlimited'
      : (MAX_ITEMS_OPTIONS.find(option => option.count === countItems)?.value ?? '500')

  // Storage stats state
  const [stats, setStats] = useState<StorageStats | null>(null)
  const [statsLoading, setStatsLoading] = useState(true)
  const [statsError, setStatsError] = useState<string | null>(null)

  // Search index state
  const [searchStatus, setSearchStatus] = useState<SearchStatusData | null>(null)
  const [rebuildingIndex, setRebuildingIndex] = useState(false)
  const rebuildPollRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // Action states
  const [clearingCache, setClearingCache] = useState(false)
  const [clearingHistory, setClearingHistory] = useState(false)
  const [showClearHistoryDialog, setShowClearHistoryDialog] = useState(false)

  // ── Load storage stats ───────────────────────────────────────────

  const loadStats = useCallback(async (isCancelled: () => boolean = () => false) => {
    setStatsLoading(true)
    setStatsError(null)
    try {
      const result = await storageApi.getStorageStats()
      if (isCancelled()) return
      setStats(result)
    } catch (err) {
      if (isCancelled()) return
      log.error({ err }, 'Failed to load storage stats')
      setStatsError(err instanceof Error ? err.message : String(err))
    } finally {
      if (!isCancelled()) setStatsLoading(false)
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    void loadStats(() => cancelled)
    return () => {
      cancelled = true
    }
  }, [loadStats])

  // ── Load search index status ────────────────────────────────────

  const loadSearchStatus = useCallback(async (isCancelled: () => boolean = () => false) => {
    try {
      const resp = await getSearchStatus()
      if (isCancelled()) return
      setSearchStatus(resp.data)
    } catch (err) {
      if (isCancelled()) return
      log.error({ err }, 'Failed to load search index status')
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    void loadSearchStatus(() => cancelled)
    return () => {
      cancelled = true
    }
  }, [loadSearchStatus])

  useEffect(() => {
    return () => {
      if (rebuildPollRef.current) clearInterval(rebuildPollRef.current)
    }
  }, [])

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

  // ── Handlers ─────────────────────────────────────────────────────

  const handleEnabledChange = (checked: boolean) => {
    setRetentionPolicy({ ...retentionPolicy, enabled: checked })
  }

  const handleRetentionDaysChange = (value: string) => {
    const days = RETENTION_DAYS_OPTIONS.find(o => o.value === value)?.days ?? 30
    setRetentionPolicy({
      ...retentionPolicy,
      rules: setByAgeRule(retentionPolicy.rules, days),
    })
  }

  const handleMaxItemsChange = (value: string) => {
    const count = resolveMaxItemsCount(value)
    setRetentionPolicy({
      ...retentionPolicy,
      rules:
        count === null
          ? clearByCountRule(retentionPolicy.rules)
          : setByCountRule(retentionPolicy.rules, count),
    })
  }

  const handleSkipPinnedChange = (checked: boolean) => {
    setRetentionPolicy({ ...retentionPolicy, skipPinned: checked })
  }

  const handleClearCache = async () => {
    setClearingCache(true)
    try {
      await storageApi.clearCache(true)
      await loadStats()
    } catch (err) {
      log.error({ err }, 'Failed to clear cache')
    } finally {
      setClearingCache(false)
    }
  }

  const handleClearHistory = async () => {
    setClearingHistory(true)
    try {
      await storageApi.clearAllClipboardHistory()
      // The history view re-queries via useLiveSearch on next mount; no Redux
      // browse list to reset anymore.
      await loadStats()
    } catch (err) {
      log.error({ err }, 'Failed to clear history')
      throw err
    } finally {
      setClearingHistory(false)
    }
  }

  const handleRebuildIndex = async () => {
    setRebuildingIndex(true)
    try {
      await triggerSearchRebuild()
      // Poll status until rebuild completes or timeout
      if (rebuildPollRef.current) clearInterval(rebuildPollRef.current)
      rebuildPollRef.current = setInterval(async () => {
        try {
          const resp = await getSearchStatus()
          setSearchStatus(resp.data)
          if (resp.data.state !== 'rebuilding') {
            if (rebuildPollRef.current) clearInterval(rebuildPollRef.current)
            rebuildPollRef.current = null
            setRebuildingIndex(false)
          }
        } catch {
          if (rebuildPollRef.current) clearInterval(rebuildPollRef.current)
          rebuildPollRef.current = null
          setRebuildingIndex(false)
        }
      }, 2000)
    } catch (err) {
      log.error({ err }, 'Failed to trigger search index rebuild')
      setRebuildingIndex(false)
    }
  }

  const handleOpenDataDir = async () => {
    try {
      await storageApi.openDataDirectory()
    } catch (err) {
      log.error({ err }, 'Failed to open data directory')
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
          onRefresh={() => {
            void loadStats()
          }}
        />
      </SettingGroup>

      {/* ── Search Index ── */}
      <SettingGroup title={t('settings.sections.storage.searchIndex.label')}>
        <SettingRow
          label={t('settings.sections.storage.searchIndex.status')}
          description={t('settings.sections.storage.searchIndex.statusDescription')}
        >
          <div className="flex items-center gap-2">
            <span
              className={`inline-block size-2 rounded-full ${
                searchStatus?.state === 'ready'
                  ? 'bg-green-500'
                  : searchStatus?.state === 'rebuilding'
                    ? 'bg-yellow-500 animate-pulse'
                    : 'bg-muted-foreground/40'
              }`}
            />
            <span className="text-sm text-muted-foreground">
              {searchStatus?.state === 'ready'
                ? t('settings.sections.storage.searchIndex.ready')
                : searchStatus?.state === 'rebuilding'
                  ? t('settings.sections.storage.searchIndex.rebuilding')
                  : t('settings.sections.storage.searchIndex.unavailable')}
            </span>
          </div>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.searchIndex.lastRebuilt')}
          description={t('settings.sections.storage.searchIndex.lastRebuiltDescription')}
        >
          <span className="text-sm text-muted-foreground tabular-nums">
            {searchStatus?.lastRebuildCompletedAtMs
              ? new Date(searchStatus.lastRebuildCompletedAtMs).toLocaleString()
              : t('settings.sections.storage.searchIndex.never')}
          </span>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.searchIndex.rebuild')}
          description={t('settings.sections.storage.searchIndex.rebuildDescription')}
        >
          <Button
            variant="outline"
            size="sm"
            onClick={handleRebuildIndex}
            disabled={rebuildingIndex || searchStatus?.state === 'rebuilding'}
          >
            {rebuildingIndex || searchStatus?.state === 'rebuilding'
              ? t('settings.sections.storage.searchIndex.rebuildingButton')
              : t('settings.sections.storage.searchIndex.rebuildButton')}
          </Button>
        </SettingRow>
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
                  {opt.count === null
                    ? t('settings.sections.storage.maxHistoryItems.unlimited')
                    : t('settings.sections.storage.maxHistoryItems.items', { count: opt.count })}
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

      {/* ── Config Backup / Migration ── */}
      <ConfigBackupGroup />

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
