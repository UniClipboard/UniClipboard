import { getVersion } from '@tauri-apps/api/app'
import { ChevronRight } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { LocalDeviceInfo } from '@/api/daemon/members'
import CopyIconButton from '@/components/device/CopyIconButton'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import { getDeviceIcon } from '@/components/device/device-utils'
import LocalSyncToggle from '@/components/device/LocalSyncToggle'
import PanelFactRow from '@/components/device/PanelFactRow'
import RebuildSpaceDialog from '@/components/device/RebuildSpaceDialog'
import StatusDot from '@/components/device/StatusDot'
import { Button } from '@/components/ui/button'
import { useSettingSelector } from '@/hooks/useSetting'
import { detectPlatformInfo } from '@/lib/platform'
import { cn } from '@/lib/utils'

interface LocalDevicePanelProps {
  localDevice: LocalDeviceInfo
  /** Total member count of the current space (including this device). */
  memberCount: number
  status?: DeviceRowStatus
  onRebuildSucceeded?: () => void
}

const LocalDevicePanel: React.FC<LocalDevicePanelProps> = ({
  localDevice,
  memberCount,
  status,
  onRebuildSucceeded,
}) => {
  const { t } = useTranslation()
  const [showResetModal, setShowResetModal] = useState(false)
  const [appVersion, setAppVersion] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    getVersion()
      .then(version => {
        if (!cancelled) setAppVersion(version)
      })
      .catch(() => {
        // Outside Tauri (plain browser dev) the API is unavailable; the
        // version row simply stays hidden.
      })
    return () => {
      cancelled = true
    }
  }, [])

  const platformLabel = getPlatformLabel()

  return (
    <div className="@container min-h-full w-full bg-muted/20">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-7 px-5 py-8 @md:px-8 @lg:py-10">
        <LocalDeviceHeader localDevice={localDevice} status={status} />

        <div className="flex min-w-0 flex-col gap-5">
          <section className="min-w-0 overflow-hidden rounded-xl border border-border/60 bg-card text-card-foreground [&>div+div]:border-t [&>div+div]:border-border/50">
            <LocalSyncToggle kind="sync" />
            <LocalSyncToggle kind="file" />
          </section>

          <details className="group overflow-hidden rounded-xl border border-border/60 bg-card text-card-foreground">
            <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-5 py-4 text-sm font-medium transition-colors hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring @md:px-6 [&::-webkit-details-marker]:hidden">
              {t('devices.panel.profile.title')}
              <ChevronRight className="size-4 text-muted-foreground transition-transform group-open:rotate-90" />
            </summary>
            <div className="flex flex-col border-t border-border/50 px-5 py-2 @md:px-6 [&>div]:py-3">
              <PanelFactRow label={t('devices.panel.fields.peerId')}>
                <span className="inline-flex min-w-0 max-w-full items-center gap-1">
                  <span
                    className="min-w-0 truncate font-mono text-xs font-medium"
                    title={localDevice.peerId}
                  >
                    {localDevice.peerId}
                  </span>
                  <CopyIconButton value={localDevice.peerId} />
                </span>
              </PanelFactRow>
              {platformLabel && (
                <PanelFactRow label={t('devices.panel.profile.platform')}>
                  <span className="text-xs font-medium">{platformLabel}</span>
                </PanelFactRow>
              )}
              {appVersion && (
                <PanelFactRow label={t('devices.panel.profile.version')}>
                  <span className="font-mono text-xs font-medium">v{appVersion}</span>
                </PanelFactRow>
              )}
              <PanelFactRow label={t('devices.panel.profile.space')}>
                <span className="text-xs font-medium">
                  {t('devices.panel.profile.memberCount', { count: memberCount })}
                </span>
              </PanelFactRow>
            </div>
          </details>
          <section
            aria-label={t('devices.panel.danger.title')}
            className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-destructive/20 bg-card px-5 py-4 text-card-foreground @md:px-6"
          >
            <div className="min-w-0 flex-1 basis-48">
              <h4 className="text-sm font-medium text-destructive">
                {t('devices.panel.danger.title')}
              </h4>
              <p className="mt-1 text-xs text-muted-foreground">
                {t('devices.panel.danger.description')}
              </p>
            </div>
            <Button variant="destructive" onClick={() => setShowResetModal(true)}>
              {t('devices.panel.danger.reset')}
            </Button>
          </section>
          {showResetModal && (
            <RebuildSpaceDialog
              onClose={() => setShowResetModal(false)}
              onRebuildSucceeded={onRebuildSucceeded}
            />
          )}
        </div>
      </div>
    </div>
  )
}

function LocalDeviceHeader({
  localDevice,
  status,
}: {
  localDevice: LocalDeviceInfo
  status?: DeviceRowStatus
}) {
  const { t } = useTranslation()
  const settingsLoaded = useSettingSelector(context => context.setting !== null)
  const syncActive = useSettingSelector(
    ({ setting }) => setting !== null && setting.sync.syncEnabled !== false
  )
  const Icon = getDeviceIcon(localDevice.deviceName)
  const needsAttention = status != null && !['online', 'paused', 'unknown'].includes(status.kind)
  return (
    <header className="flex items-center gap-4">
      <div className="flex size-16 shrink-0 items-center justify-center rounded-xl border border-border/60 bg-card text-foreground">
        {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
        <Icon className="size-8" strokeWidth={1.5} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <h3
            title={localDevice.deviceName}
            className="truncate text-2xl font-semibold tracking-tight text-foreground"
          >
            {localDevice.deviceName}
          </h3>
          <span className="shrink-0 rounded-md bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
            {t('devices.panel.localBadge')}
          </span>
        </div>
        <p className="mt-2 flex items-center gap-2 text-xs">
          <StatusDot tone={needsAttention ? 'warning' : syncActive ? 'success' : 'off'} />
          <span
            className={cn(
              'font-medium',
              needsAttention
                ? 'text-warning'
                : syncActive
                  ? 'text-success'
                  : 'text-muted-foreground'
            )}
          >
            {status?.label ??
              (!settingsLoaded
                ? t('common.loading')
                : syncActive
                  ? t('devices.thisDevice.syncActive')
                  : t('devices.thisDevice.syncPaused'))}
          </span>
        </p>
      </div>
    </header>
  )
}

export default React.memo(LocalDevicePanel)

// ────────────────────────────────────────────────────────────────
// Local helpers (file-private)
// ────────────────────────────────────────────────────────────────

function getPlatformLabel(): string | null {
  const info = detectPlatformInfo()
  if (info.isMac) return 'macOS'
  if (info.isWindows) return 'Windows'
  if (info.isLinux) return 'Linux'
  return null
}
