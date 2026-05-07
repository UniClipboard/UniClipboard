/**
 * MobileShortcutDevicesPanel —— iPhone 设备 panel(方案 F · macOS-native list)。
 *
 * # 设计语言
 *
 * 取消 ALL CAPS 标题 / 虚线加号卡片 / 2 列网格 / 大尺寸 iOS 风格 icon。
 * 改为 macOS 系统设置 / Tailscale 风格的 grouped-list:
 *
 *   ┌─ Section header ──────────────────────────────────────────┐
 *   │ iPhone (Shortcut)                       [Configure] [+ Add] │
 *   │ Listening on 192.168.1.5:42720                              │
 *   ├─ List container ────────────────────────────────────────────┤
 *   │ 📱 My iPhone              5m ago · .42                  ⌫  │
 *   │ 📱 Test iPhone            never                          ⌫  │
 *   └─────────────────────────────────────────────────────────────┘
 *
 * 状态文案在 header 副标题里(不再单独占一个 card),Configure 和 Add 都
 * 收在 header 右侧。设备列表是单个圆角容器 + divide-y rows,每行 ~52px。
 *
 * # 不变量
 * - settings 真相源仍在 SettingsSheet
 * - LAN 安全告警 / 重启提示 / 5 行设置都在 Sheet 内,不在 panel 主区域
 * - revoke 流程不变
 */

import { Plus, RefreshCw, Settings2, Smartphone, Trash2 } from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import AddMobileShortcutDeviceDialog from './AddMobileShortcutDeviceDialog'
import MobileShortcutCredentialModal from './MobileShortcutCredentialModal'
import MobileShortcutSettingsSheet from './MobileShortcutSettingsSheet'
import {
  isMobileSyncError,
  listMobileDevices,
  revokeMobileDevice,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-shortcut-panel')

const MobileShortcutDevicesPanel: React.FC = () => {
  const { t } = useTranslation()

  const [settings, setSettings] = useState<MobileSyncSettingsView | null>(null)
  const [devices, setDevices] = useState<MobileDeviceView[]>([])
  const [devicesError, setDevicesError] = useState<string | null>(null)

  const [settingsSheetOpen, setSettingsSheetOpen] = useState(false)
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  const [credentialPayload, setCredentialPayload] = useState<RegisterMobileDeviceResult | null>(
    null
  )

  const [revokeTarget, setRevokeTarget] = useState<MobileDeviceView | null>(null)
  const [revokeBusy, setRevokeBusy] = useState(false)

  const translate = useCallback((err: unknown): string => translateMobileSyncError(t, err), [t])

  const loadDevices = useCallback(async () => {
    try {
      const list = await listMobileDevices()
      setDevices(list)
      setDevicesError(null)
    } catch (err) {
      log.error({ err }, 'failed to list mobile devices')
      setDevicesError(translate(err))
    }
  }, [translate])

  useEffect(() => {
    void loadDevices()
  }, [loadDevices])

  const handleOpenAddDialog = useCallback(() => {
    if (!settings?.lanListenEnabled) {
      toast.error(t('devices.mobileShortcut.errors.lanListenerDisabled'))
      return
    }
    setAddDialogOpen(true)
  }, [settings?.lanListenEnabled, t])

  const handleAddSuccess = useCallback(
    (result: RegisterMobileDeviceResult) => {
      setCredentialPayload(result)
      void loadDevices()
    },
    [loadDevices]
  )

  const handleRevokeConfirm = useCallback(async () => {
    if (!revokeTarget) return
    setRevokeBusy(true)
    try {
      await revokeMobileDevice(revokeTarget.deviceId)
      toast.success(t('devices.mobileShortcut.revoke.confirmTitle', { label: revokeTarget.label }))
      setRevokeTarget(null)
      await loadDevices()
    } catch (err) {
      log.error({ err, deviceId: revokeTarget.deviceId }, 'failed to revoke device')
      toast.error(translate(err))
    } finally {
      setRevokeBusy(false)
    }
  }, [loadDevices, revokeTarget, t, translate])

  // ── Derived ──────────────────────────────────────────────────────────
  const enabled = settings?.enabled ?? false
  const lanListenEnabled = settings?.lanListenEnabled ?? false
  const lanListenerError = settings?.lanListenerError ?? null
  const addDisabled = !enabled || !lanListenEnabled || lanListenerError != null

  // 状态推导:四态色 + 五态文案。lanListenerError 优先级最高 —— bind 失败
  // 是用户最需要立刻看见的可操作信号(端口占用 / IP 不存在等),如果只
  // fallback 到 "lanOff" / "pending" 用户会以为是 daemon 没起来,实际是
  // 起来了但没绑成功。
  const subtitleKind: 'ok' | 'warn' | 'muted' | 'error' =
    lanListenerError != null
      ? 'error'
      : !enabled
        ? 'muted'
        : !lanListenEnabled
          ? 'warn'
          : settings?.currentLanUrl
            ? 'ok'
            : 'warn'

  const subtitleText = (() => {
    if (lanListenerError != null)
      return t('devices.mobileShortcut.statusBar.bindFailed', { reason: lanListenerError })
    if (!enabled) return t('devices.mobileShortcut.statusBar.disabled')
    if (!lanListenEnabled) return t('devices.mobileShortcut.statusBar.lanOff')
    if (settings?.currentLanUrl)
      return t('devices.mobileShortcut.statusBar.listening', { url: settings.currentLanUrl })
    return t('devices.mobileShortcut.statusBar.pending')
  })()

  return (
    <>
      <section>
        {/* ── Section header ─────────────────────────────────────── */}
        <div className="mb-2 flex items-end justify-between gap-3 px-1">
          <div className="min-w-0">
            <h3 className="text-base font-semibold text-foreground">
              {t('devices.mobileShortcut.title')}
            </h3>
            <p
              className={`mt-0.5 text-xs leading-tight ${
                subtitleKind === 'error'
                  ? 'text-destructive'
                  : subtitleKind === 'warn'
                    ? 'text-amber-600 dark:text-amber-400'
                    : 'text-muted-foreground'
              }`}
            >
              {subtitleKind === 'ok' ? (
                <>
                  <span className="text-emerald-600 dark:text-emerald-400">●</span>{' '}
                  <span className="font-mono">{subtitleText}</span>
                </>
              ) : subtitleKind === 'error' ? (
                <>
                  <span>●</span> {subtitleText}
                </>
              ) : (
                subtitleText
              )}
            </p>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            <Button variant="ghost" size="sm" onClick={() => setSettingsSheetOpen(true)}>
              <Settings2 className="h-3.5 w-3.5" />
              {t('devices.mobileShortcut.configure')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleOpenAddDialog}
              disabled={addDisabled}
              title={
                addDisabled ? t('devices.mobileShortcut.errors.lanListenerDisabled') : undefined
              }
            >
              <Plus className="h-3.5 w-3.5" />
              {t('devices.mobileShortcut.list.addButton')}
            </Button>
          </div>
        </div>

        {/* ── List container ─────────────────────────────────────── */}
        {devicesError ? (
          <Alert variant="destructive">
            <AlertDescription className="flex items-center gap-3">
              <span className="flex-1">{devicesError}</span>
              <Button variant="ghost" size="icon-sm" onClick={() => void loadDevices()}>
                <RefreshCw className="h-4 w-4" />
              </Button>
            </AlertDescription>
          </Alert>
        ) : (
          <div className="overflow-hidden rounded-xl border border-border/60 bg-card">
            {devices.length === 0 ? (
              <EmptyRow t={t} />
            ) : (
              <ul className="divide-y divide-border/50">
                {devices.map(device => (
                  <DeviceRow
                    key={device.deviceId}
                    device={device}
                    onRevoke={() => setRevokeTarget(device)}
                  />
                ))}
              </ul>
            )}
          </div>
        )}
      </section>

      <MobileShortcutSettingsSheet
        open={settingsSheetOpen}
        onOpenChange={setSettingsSheetOpen}
        onSettingsChange={setSettings}
      />

      <AlertDialog
        open={!!revokeTarget}
        onOpenChange={open => !open && !revokeBusy && setRevokeTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('devices.mobileShortcut.revoke.confirmTitle', {
                label: revokeTarget?.label ?? '',
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileShortcut.revoke.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={revokeBusy}>
              {t('devices.mobileShortcut.revoke.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={handleRevokeConfirm}
              disabled={revokeBusy}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t('devices.mobileShortcut.revoke.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AddMobileShortcutDeviceDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSuccess={handleAddSuccess}
      />

      <MobileShortcutCredentialModal
        payload={credentialPayload}
        onClose={() => setCredentialPayload(null)}
      />
    </>
  )
}

// ─── Subcomponents ─────────────────────────────────────────────────────

interface DeviceRowProps {
  device: MobileDeviceView
  onRevoke: () => void
}

const DeviceRow: React.FC<DeviceRowProps> = ({ device, onRevoke }) => {
  const { t } = useTranslation()
  const lastSeen = useMemo(() => {
    if (device.lastSeenAtMs == null) {
      return t('devices.mobileShortcut.list.lastSeen.never')
    }
    return formatRelativeMs(device.lastSeenAtMs)
  }, [device.lastSeenAtMs, t])

  return (
    <li className="group flex items-center gap-3 px-4 py-3 transition-colors hover:bg-muted/30">
      <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-blue-500/10 text-blue-600 dark:text-blue-400">
        <Smartphone className="h-5 w-5" />
      </div>

      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-foreground">{device.label}</p>
        <p className="mt-0.5 truncate text-xs text-muted-foreground">
          {lastSeen}
          {device.lastSeenIp && (
            <>
              <span className="mx-1.5 text-muted-foreground/50">·</span>
              <span className="font-mono">{device.lastSeenIp}</span>
            </>
          )}
        </p>
      </div>

      <Button
        variant="ghost"
        size="icon-sm"
        className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
        onClick={onRevoke}
        aria-label={t('devices.mobileShortcut.revoke.confirm')}
        title={t('devices.mobileShortcut.revoke.confirm')}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </li>
  )
}

interface EmptyRowProps {
  t: ReturnType<typeof useTranslation>['t']
}

const EmptyRow: React.FC<EmptyRowProps> = ({ t }) => (
  <div className="px-4 py-4 text-center text-xs text-muted-foreground">
    {t('devices.mobileShortcut.list.empty.title')}
  </div>
)

// ─── Helpers ───────────────────────────────────────────────────────────

function formatRelativeMs(timestampMs: number): string {
  const diffMs = Date.now() - timestampMs
  const diffMins = Math.round(diffMs / 60_000)
  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m`
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h`
  return `${Math.floor(diffMins / 1440)}d`
}

function translateMobileSyncError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileShortcut.errors.facadeUnavailable')
      case 'LAN_LISTENER_DISABLED':
        return t('devices.mobileShortcut.errors.lanListenerDisabled')
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileShortcut.errors.deviceNotFound')
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileShortcut.errors.persistenceFailed', { message: e.message })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileShortcut.errors.settingsLoadFailed', { message: e.message })
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileShortcut.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileShortcut.errors.unknown', { message })
}

export const __test__ = { translateMobileSyncError, formatRelativeMs }

export default MobileShortcutDevicesPanel
