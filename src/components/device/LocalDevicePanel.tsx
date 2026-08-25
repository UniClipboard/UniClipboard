/**
 * LocalDevicePanel: detail pane for this device in the devices
 * master-detail layout ("profile + global policy" treatment).
 *
 * Two sections under the identity header:
 *  - 身份档案: full peer id (copyable), platform, app version, space size.
 *  - 全局同步策略: the two most-used global switches (sync, file sync) as
 *    inline toggles that write directly through the setting context — no jump
 *    into Settings. File sync depends on global sync (mirrors SyncSection).
 */

import { getVersion } from '@tauri-apps/api/app'
import { ArrowRightLeft } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { LocalDeviceInfo } from '@/api/daemon/members'
import CopyIconButton from '@/components/device/CopyIconButton'
import { getDeviceIcon } from '@/components/device/device-utils'
import PanelFactRow from '@/components/device/PanelFactRow'
import StatusDot from '@/components/device/StatusDot'
import SwitchSpaceDialog from '@/components/device/SwitchSpaceDialog'
import AddSpaceDialog from '@/components/spaces/AddSpaceDialog'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { detectPlatformInfo } from '@/lib/platform'
import { cn } from '@/lib/utils'

const log = createLogger('local-device-panel')

interface LocalDevicePanelProps {
  localDevice: LocalDeviceInfo
  /** Total member count of the current space (including this device). */
  memberCount: number
}

const LocalDevicePanel: React.FC<LocalDevicePanelProps> = ({ localDevice, memberCount }) => {
  const { t } = useTranslation()
  const { setting, updateSyncSetting, updateFileSyncSetting } = useSetting()
  const [addSpaceOpen, setAddSpaceOpen] = useState(false)
  const [legacyRecoveryOpen, setLegacyRecoveryOpen] = useState(false)
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

  const syncEnabled = setting?.sync.syncEnabled !== false
  const fileSyncEnabled = setting?.fileSync?.fileSyncEnabled !== false
  const syncActive = syncEnabled
  const platformLabel = getPlatformLabel()
  const Icon = getDeviceIcon(localDevice.deviceName)

  const handleSyncChange = (checked: boolean) => {
    updateSyncSetting({ syncEnabled: checked }).catch(err => {
      log.error({ err }, 'failed to update sync setting')
    })
  }

  const handleFileSyncChange = (checked: boolean) => {
    updateFileSyncSetting({ fileSyncEnabled: checked }).catch(err => {
      log.error({ err }, 'failed to update file sync setting')
    })
  }

  return (
    <div className="@container flex min-h-full w-full flex-col bg-background">
      <header className="flex items-center gap-3 border-b border-border/40 bg-card/40 px-5 py-4 @md:px-6">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border bg-success/10 text-success">
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
          <Icon className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-lg font-semibold text-foreground">
              {localDevice.deviceName}
            </h3>
            <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[9px] font-medium text-muted-foreground">
              {t('devices.panel.localBadge')}
            </span>
          </div>
          <p className="mt-1 flex items-center gap-2 text-[11px]">
            <StatusDot tone={syncActive ? 'success' : 'off'} />
            <span
              className={cn('font-medium', syncActive ? 'text-success' : 'text-muted-foreground')}
            >
              {syncActive ? t('devices.thisDevice.syncActive') : t('devices.thisDevice.syncPaused')}
            </span>
          </p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="shrink-0"
          aria-label={t('spaces.actions.join')}
          title={t('spaces.actions.join')}
          onClick={() => setAddSpaceOpen(true)}
        >
          <ArrowRightLeft className="size-3.5" />
          <span className="hidden @lg:inline">{t('spaces.actions.join')}</span>
        </Button>
      </header>

      <div className="grid min-w-0 content-start @3xl:flex-1 @3xl:content-stretch @3xl:grid-cols-[minmax(16rem,0.82fr)_minmax(22rem,1.18fr)]">
        <section className="min-w-0 px-5 pt-5 pb-3 @md:px-6 @3xl:py-5">
          <h4 className="text-xs font-semibold text-foreground/80">
            {t('devices.panel.profile.title')}
          </h4>
          <div className="mt-2.5 flex flex-col gap-0.5 [&>div]:border-0 [&>div]:py-2">
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
              <span className="flex min-w-0 items-center gap-2 text-xs font-medium">
                <span>{t('devices.panel.profile.memberCount', { count: memberCount })}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={() => setLegacyRecoveryOpen(true)}
                >
                  {t('spaces.actions.legacyRecovery')}
                </Button>
              </span>
            </PanelFactRow>
          </div>
        </section>

        <section className="min-w-0 px-5 pt-3 pb-5 @md:px-6 @3xl:py-5">
          <div className="flex h-7 items-center">
            <h4 className="text-[13px] font-semibold text-foreground/90">
              {t('devices.panel.policies.title')}
            </h4>
          </div>
          <div className="mt-2.5 flex flex-col gap-1">
            <ToggleRow
              title={t('devices.panel.policies.syncEnabled.title')}
              description={t('devices.panel.policies.syncEnabled.description')}
              checked={syncEnabled}
              onCheckedChange={handleSyncChange}
            />
            <ToggleRow
              title={t('devices.panel.policies.fileSync.title')}
              description={t('devices.panel.policies.fileSync.description')}
              checked={syncEnabled && fileSyncEnabled}
              onCheckedChange={handleFileSyncChange}
              disabled={!syncEnabled}
            />
          </div>
        </section>
      </div>

      <AddSpaceDialog open={addSpaceOpen} onOpenChange={setAddSpaceOpen} />
      <SwitchSpaceDialog open={legacyRecoveryOpen} onOpenChange={setLegacyRecoveryOpen} />
    </div>
  )
}

export default LocalDevicePanel

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

interface ToggleRowProps {
  title: string
  description: string
  checked: boolean
  onCheckedChange: (checked: boolean) => void
  disabled?: boolean
}

const ToggleRow: React.FC<ToggleRowProps> = ({
  title,
  description,
  checked,
  onCheckedChange,
  disabled,
}) => (
  <div
    className={cn(
      'flex min-h-14 min-w-0 items-center gap-4 rounded-md p-3.5 transition-colors hover:bg-muted/20',
      disabled && 'opacity-55'
    )}
  >
    <div className="min-w-0 flex-1">
      <p className="text-sm font-medium text-foreground">{title}</p>
      <p className="mt-0.5 text-[11px] leading-snug text-muted-foreground">{description}</p>
    </div>
    <Switch
      className="shrink-0"
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
    />
  </div>
)
