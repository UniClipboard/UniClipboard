/**
 * MobileSyncDevicesPanel —— 移动设备同步 panel(方案 F · macOS-native list)。
 *
 * 概念:本特性的 LAN 监听跑的是 SyncClipboard 协议(HTTP + Basic Auth + 4
 * 个固定路由),凡是兼容该协议的客户端均可接入(iOS 快捷指令是默认入口,
 * Android / 鸿蒙等只要实现该协议就能复用同一组凭据)。命名上 panel/dialog
 * /sheet 全部使用平台无关的 "MobileSync" 前缀;仅 credential modal 内部
 * 按平台 tab 展示具体接入步骤。
 *
 * # 设计语言
 *
 * macOS 系统设置 / Tailscale 风格的 grouped-list:
 *
 *   ┌─ Section header ────────────────────────────────────────────┐
 *   │ Mobile Sync                            [Configure] [+ Add]  │
 *   ├─ List container ────────────────────────────────────────────┤
 *   │ 📱 My iPhone              5m ago · .42                  ⌫  │
 *   │    mobile_a1b2c3d4                                          │
 *   │ 📱 Pixel                  never                          ⌫  │
 *   │    mobile_e5f6g7h8                                          │
 *   └─────────────────────────────────────────────────────────────┘
 *
 * 每行双行布局: 主行 label, 副行 username (Basic Auth 账号), 与 macOS
 * 系统设置 / Tailscale 的 "primary text + secondary text" 风格一致。
 * username 暴露在 UI 上是有意决策 —— label 可重命名重复, username 是
 * server 端稳定的识别主键, 帮用户区分多台同名设备。
 *
 * Header 只放标题 + Configure + Add(状态文案/监听 URL/bind 错误均收
 * 进 SettingsSheet,主页面保持极简)。设备列表是单个圆角容器 + divide-y
 * rows,每行 ~52px。
 *
 * # 不变量
 * - settings 真相源仍在 SettingsSheet
 * - LAN 安全告警 / 重启提示 / 5 行设置 / bind 错误都在 Sheet 内,
 *   不在 panel 主区域
 * - revoke 流程不变
 */

import { KeyRound, Plus, RefreshCw, Settings2, Smartphone, Trash2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import AddMobileSyncDeviceDialog from './AddMobileSyncDeviceDialog'
import MobileSyncCredentialModal from './MobileSyncCredentialModal'
import MobileSyncSettingsSheet from './MobileSyncSettingsSheet'
import RotatedPasswordModal from './RotatedPasswordModal'
import RotateMobilePasswordDialog from './RotateMobilePasswordDialog'
import {
  isMobileSyncError,
  listMobileDevices,
  revokeMobileDevice,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
  type RotateMobilePasswordResult,
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

const log = createLogger('mobile-sync-panel')

const MobileSyncDevicesPanel: React.FC = () => {
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

  const [rotateTarget, setRotateTarget] = useState<MobileDeviceView | null>(null)
  const [rotatedPayload, setRotatedPayload] = useState<RotateMobilePasswordResult | null>(null)

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
      toast.error(t('devices.mobileSync.errors.lanListenerDisabled'))
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

  const handleRotateSuccess = useCallback(
    (result: RotateMobilePasswordResult) => {
      setRotatedPayload(result)
      void loadDevices()
    },
    [loadDevices]
  )

  const handleRevokeConfirm = useCallback(async () => {
    if (!revokeTarget) return
    setRevokeBusy(true)
    try {
      await revokeMobileDevice(revokeTarget.deviceId)
      toast.success(t('devices.mobileSync.revoke.confirmTitle', { label: revokeTarget.label }))
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

  return (
    <>
      <section>
        {/* ── Section header ─────────────────────────────────────── */}
        <div className="mb-2 flex items-end justify-between gap-3 px-1">
          <div className="min-w-0">
            <h3 className="text-base font-semibold text-foreground">
              {t('devices.mobileSync.title')}
            </h3>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            <Button variant="ghost" size="sm" onClick={() => setSettingsSheetOpen(true)}>
              <Settings2 className="h-3.5 w-3.5" />
              {t('devices.mobileSync.configure')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleOpenAddDialog}
              disabled={addDisabled}
              title={addDisabled ? t('devices.mobileSync.errors.lanListenerDisabled') : undefined}
            >
              <Plus className="h-3.5 w-3.5" />
              {t('devices.mobileSync.list.addButton')}
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
                    onRotate={() => setRotateTarget(device)}
                  />
                ))}
              </ul>
            )}
          </div>
        )}
      </section>

      <MobileSyncSettingsSheet
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
              {t('devices.mobileSync.revoke.confirmTitle', {
                label: revokeTarget?.label ?? '',
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileSync.revoke.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={revokeBusy}>
              {t('devices.mobileSync.revoke.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={handleRevokeConfirm}
              disabled={revokeBusy}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t('devices.mobileSync.revoke.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AddMobileSyncDeviceDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSuccess={handleAddSuccess}
      />

      <MobileSyncCredentialModal
        payload={credentialPayload}
        onClose={() => setCredentialPayload(null)}
      />

      <RotateMobilePasswordDialog
        open={rotateTarget !== null}
        onOpenChange={open => {
          if (!open) setRotateTarget(null)
        }}
        device={
          rotateTarget ? { deviceId: rotateTarget.deviceId, label: rotateTarget.label } : null
        }
        onSuccess={handleRotateSuccess}
      />

      <RotatedPasswordModal payload={rotatedPayload} onClose={() => setRotatedPayload(null)} />
    </>
  )
}

// ─── Subcomponents ─────────────────────────────────────────────────────

interface DeviceRowProps {
  device: MobileDeviceView
  onRevoke: () => void
  onRotate: () => void
}

const DeviceRow: React.FC<DeviceRowProps> = ({ device, onRevoke, onRotate }) => {
  const { t } = useTranslation()

  return (
    <li className="group flex items-center gap-3 px-4 py-3 transition-colors hover:bg-muted/30">
      <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-blue-500/10 text-blue-600 dark:text-blue-400">
        <Smartphone className="h-5 w-5" />
      </div>

      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-foreground">{device.label}</p>
        <p className="truncate font-mono text-xs text-muted-foreground">{device.username}</p>
      </div>

      <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onRotate}
          aria-label={t('devices.mobileSync.rotate.button')}
          title={t('devices.mobileSync.rotate.button')}
        >
          <KeyRound className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onRevoke}
          aria-label={t('devices.mobileSync.revoke.confirm')}
          title={t('devices.mobileSync.revoke.confirm')}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </li>
  )
}

interface EmptyRowProps {
  t: ReturnType<typeof useTranslation>['t']
}

const EmptyRow: React.FC<EmptyRowProps> = ({ t }) => (
  <div className="px-4 py-4 text-center text-xs text-muted-foreground">
    {t('devices.mobileSync.list.empty.title')}
  </div>
)

// ─── Helpers ───────────────────────────────────────────────────────────

function translateMobileSyncError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      case 'LAN_LISTENER_DISABLED':
        return t('devices.mobileSync.errors.lanListenerDisabled')
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileSync.errors.deviceNotFound')
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message })
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}

export const __test__ = { translateMobileSyncError }

export default MobileSyncDevicesPanel
