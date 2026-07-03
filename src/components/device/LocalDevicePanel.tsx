/**
 * LocalDevicePanel: detail pane for this device in the devices
 * master-detail layout ("profile + global policy" treatment).
 *
 * Two sections under the identity header:
 *  - 身份档案: full peer id (copyable), platform, app version, space size.
 *  - 全局同步策略: the two most-used global switches (auto sync, file
 *    sync) as inline toggles that write directly through the setting
 *    context — no jump into Settings. File sync depends on auto sync, so
 *    its toggle is disabled while auto sync is off (mirrors SyncSection).
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
  const [switchSpaceOpen, setSwitchSpaceOpen] = useState(false)
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

  const autoSyncEnabled = setting?.sync.autoSync !== false
  const fileSyncEnabled = setting?.fileSync?.fileSyncEnabled !== false
  const syncActive = autoSyncEnabled
  const platformLabel = getPlatformLabel()
  const Icon = getDeviceIcon(localDevice.deviceName)

  const handleAutoSyncChange = (checked: boolean) => {
    updateSyncSetting({ autoSync: checked }).catch(err => {
      log.error({ err }, 'failed to update auto sync setting')
    })
  }

  const handleFileSyncChange = (checked: boolean) => {
    updateFileSyncSetting({ fileSyncEnabled: checked }).catch(err => {
      log.error({ err }, 'failed to update file sync setting')
    })
  }

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-8 py-8">
      {/* ── header ─────────────────────────────────────────────── */}
      <div className="flex items-start gap-4">
        <div className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-success/10 text-success">
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
          <Icon className="size-6" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-xl font-semibold tracking-tight text-foreground">
              {localDevice.deviceName}
            </h3>
            <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
              {t('devices.panel.localBadge')}
            </span>
          </div>
          <p className="mt-1.5 flex items-center gap-2 text-xs">
            <StatusDot tone={syncActive ? 'success' : 'off'} />
            <span
              className={cn('font-medium', syncActive ? 'text-success' : 'text-muted-foreground')}
            >
              {syncActive ? t('devices.thisDevice.syncActive') : t('devices.thisDevice.syncPaused')}
            </span>
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={() => setSwitchSpaceOpen(true)}
        >
          <ArrowRightLeft className="size-3.5" />
          {t('devices.switchSpace.button')}
        </Button>
      </div>

      {/* ── identity profile ───────────────────────────────────── */}
      <section>
        <h4 className="pb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
          {t('devices.panel.profile.title')}
        </h4>
        <div className="flex flex-col border-y border-border/50">
          <PanelFactRow label={t('devices.panel.fields.peerId')}>
            <span className="truncate font-mono text-xs font-medium" title={localDevice.peerId}>
              {localDevice.peerId}
            </span>
            <CopyIconButton value={localDevice.peerId} />
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
      </section>

      {/* ── global sync policies ───────────────────────────────── */}
      <section>
        <h4 className="pb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
          {t('devices.panel.policies.title')}
        </h4>
        <div className="flex flex-col">
          <ToggleRow
            title={t('devices.panel.policies.autoSync.title')}
            description={t('devices.panel.policies.autoSync.description')}
            checked={autoSyncEnabled}
            onCheckedChange={handleAutoSyncChange}
          />
          <ToggleRow
            title={t('devices.panel.policies.fileSync.title')}
            description={t('devices.panel.policies.fileSync.description')}
            checked={autoSyncEnabled && fileSyncEnabled}
            onCheckedChange={handleFileSyncChange}
            disabled={!autoSyncEnabled}
          />
        </div>
      </section>

      <SwitchSpaceDialog open={switchSpaceOpen} onOpenChange={setSwitchSpaceOpen} />
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
  <div className="flex items-center gap-4 border-b border-border/40 py-3 last:border-b-0">
    <div className="min-w-0 flex-1">
      <p className="text-sm font-medium text-foreground">{title}</p>
      <p className="mt-0.5 truncate text-[11px] leading-snug text-muted-foreground">
        {description}
      </p>
    </div>
    <Switch checked={checked} onCheckedChange={onCheckedChange} disabled={disabled} />
  </div>
)
