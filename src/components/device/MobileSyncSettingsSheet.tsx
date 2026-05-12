/**
 * # 为什么需要这个组件
 *
 * 把 mobile sync 配置(5 行 SettingsRow + LAN 安全告警 modal)从设备主面板
 * 拆出来塞进右侧 Sheet,让 DevicesPage 主体回到"管设备"职责,配置走二级
 * 抽屉。设备列表区域不再被配置项目挤占。
 *
 * # 关键不变量
 *
 * - LAN 安全告警:开启 LAN 监听仍弹 AlertDialog(嵌套在 Sheet 内,Radix
 *   在 portal 层正确堆叠)
 * - port: 本地 portDraft + onBlur 提交,避免每键击都触发 update_settings
 * - bindIp: `BIND_IP_AUTO_SENTINEL` ↔ null 互转(Auto 选项)
 *
 * # 改造要点 (phase 6, 2026-05-11)
 *
 * 旧版在 enabled/lanListen/port 任一变化后弹 amber "请重启" 横幅,
 * 用户必须点"立即重启" → 等 App 重启完才能继续。phase 1-5 把 daemon
 * 改成 LAN listener 即时切换 (controller.apply),本组件随之删:
 *  - `restartRequired` / `restartDismissed` state
 *  - `handleRestart` (invokeWithTrace('restart_app'))
 *  - amber 横幅 JSX
 *
 * 改为 `applySettingsUpdate` 成功后立即 `toast.success(applied)` 即时反馈。
 * 仅当 daemon 端 lifecycle adapter 报 bind 失败时,toast 改为 error +
 * 透传 reason (走 result.lanListenerBindError)。NetworkSection 的 iroh
 * 字段仍走 restart_app (Pitfall 3 / 10 真约束),与本组件独立。
 *
 * # 数据所有权
 *
 * 本组件持有 settings / lanInterfaces 等 mobile sync 相关 state。通过
 * `onSettingsChange` 回调把最新 settings 视图给父 panel,驱动状态条文案
 * 与"Add"按钮 disable 条件。
 */

import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  deriveListenUrl,
  getMobileSyncSettings,
  isMobileSyncError,
  listMobileLanInterfaces,
  updateMobileSyncSettings,
  type LanInterfaceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
} from '@/api/tauri-command/mobile_sync'
import { Input, Switch } from '@/components/ui'
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
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-sync-settings-sheet')

/**
 * Radix Select 禁止 SelectItem.value 为空串(空串保留为"无选中"哨兵)。
 * 用非空 sentinel 表示"自动 / 走 daemon 默认 127.0.0.1",在 boundary
 * 处与 facade 的 null 互转。同原 MobileSyncDevicesPanel 设计。
 */
const BIND_IP_AUTO_SENTINEL = '__auto__'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /**
   * 把最新 settings 视图回调给父 panel。null 表示加载失败/首次未就绪。
   * 父 panel 用它驱动状态条文案 + Add 按钮 disabled。
   */
  onSettingsChange?: (settings: MobileSyncSettingsView | null) => void
}

const MobileSyncSettingsSheet: React.FC<Props> = ({ open, onOpenChange, onSettingsChange }) => {
  const { t } = useTranslation()

  // ── State ────────────────────────────────────────────────────────────
  const [settings, setSettings] = useState<MobileSyncSettingsView | null>(null)
  const [settingsError, setSettingsError] = useState<string | null>(null)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])
  const [pendingLanEnable, setPendingLanEnable] = useState(false)
  const [portDraft, setPortDraft] = useState<string>('')

  // ── Helpers ──────────────────────────────────────────────────────────
  const translate = useCallback((err: unknown): string => translateMobileSyncError(t, err), [t])

  // ── Loaders ──────────────────────────────────────────────────────────
  const loadSettings = useCallback(async () => {
    try {
      const view = await getMobileSyncSettings()
      setSettings(view)
      setSettingsError(null)
      setPortDraft(view.lanPort != null ? String(view.lanPort) : '')
      onSettingsChange?.(view)
    } catch (err) {
      log.error({ err }, 'failed to load mobile sync settings')
      setSettingsError(translate(err))
      onSettingsChange?.(null)
    }
  }, [onSettingsChange, translate])

  const loadLanInterfaces = useCallback(async () => {
    try {
      const list = await listMobileLanInterfaces()
      setLanInterfaces(list)
    } catch (err) {
      log.warn({ err }, 'failed to list LAN interfaces')
      toast.error(translate(err))
    }
  }, [translate])

  // 初次挂载就拉一次,让父 panel 状态条立刻有数据;Sheet 关闭后下次打开
  // 不重新 mount,所以只跑一次;真要刷新走 facade mutation 触发的 reload。
  useEffect(() => {
    void loadSettings()
    void loadLanInterfaces()
  }, [loadSettings, loadLanInterfaces])

  // ── Mutators ─────────────────────────────────────────────────────────
  const applySettingsUpdate = useCallback(
    async (patch: Parameters<typeof updateMobileSyncSettings>[0]) => {
      setSettingsBusy(true)
      try {
        const result = await updateMobileSyncSettings(patch)
        setSettings(prev => {
          const next = prev
            ? {
                ...prev,
                enabled: result.enabled,
                lanListenEnabled: result.lanListenEnabled,
                lanAdvertiseIp: result.lanAdvertiseIp,
                lanPort: result.lanPort,
              }
            : prev
          if (next) onSettingsChange?.(next)
          return next
        })
        setPortDraft(result.lanPort != null ? String(result.lanPort) : '')
        // phase 6 起,daemon 装了 LAN lifecycle controller, listener 是即时
        // 切换的, restart_required 永远 false。toast 即时反馈即可,不再
        // 弹"请重启"横幅(NetworkSection 的 iroh 字段仍走 restart, 与本
        // 路径独立)。
        //
        // 但 "落盘成功" ≠ "listener 起来了" —— bind 失败时 facade 会把
        // reason 透传到 lanListenerBindError, 这里改 toast.error 让用户立刻
        // 知道:设置改了, 但 listener 没起,iPhone 还是连不上。完整 status
        // bar 走下面的 loadSettings → lanListenerError 路径展示。
        if (result.lanListenerBindError) {
          log.warn(
            { reason: result.lanListenerBindError, patch },
            'settings saved but LAN listener bind failed'
          )
          toast.error(
            t('devices.mobileSync.feedback.applyFailed', {
              message: result.lanListenerBindError,
            })
          )
        } else {
          toast.success(t('devices.mobileSync.feedback.applied'))
        }
        // lanListenerError 等运行时字段由 daemon 写入,update 返回值只含
        // 持久化字段;需 reload 拿最新视图。
        await loadSettings()
      } catch (err) {
        log.error({ err, patch }, 'failed to update mobile sync settings')
        toast.error(translate(err))
      } finally {
        setSettingsBusy(false)
      }
    },
    [loadSettings, onSettingsChange, t, translate]
  )

  const handleEnabledToggle = useCallback(
    (next: boolean) => void applySettingsUpdate({ enabled: next }),
    [applySettingsUpdate]
  )

  const handleLanListenToggleRequest = useCallback(
    (next: boolean) => {
      if (!next) {
        void applySettingsUpdate({ lanListenEnabled: false })
      } else {
        setPendingLanEnable(true)
      }
    },
    [applySettingsUpdate]
  )

  const handleLanWarningConfirm = useCallback(() => {
    setPendingLanEnable(false)
    void applySettingsUpdate({ lanListenEnabled: true })
  }, [applySettingsUpdate])

  const handleBindIpChange = useCallback(
    (value: string) => {
      void applySettingsUpdate({
        lanAdvertiseIp: value === BIND_IP_AUTO_SENTINEL ? null : value,
      })
    },
    [applySettingsUpdate]
  )

  const handlePortBlur = useCallback(() => {
    if (!settings) return
    const trimmed = portDraft.trim()
    if (trimmed === '') {
      if (settings.lanPort != null) {
        void applySettingsUpdate({ lanPort: null })
      }
      return
    }
    const parsed = Number(trimmed)
    if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) {
      setPortDraft(settings.lanPort != null ? String(settings.lanPort) : '')
      toast.error(
        t('devices.mobileSync.errors.invalidLanParameter', {
          reason: t('devices.mobileSync.lanListener.port.label'),
        })
      )
      return
    }
    if (parsed !== settings.lanPort) {
      void applySettingsUpdate({ lanPort: parsed })
    }
  }, [applySettingsUpdate, portDraft, settings, t])

  // ── Derived ──────────────────────────────────────────────────────────
  const enabled = settings?.enabled ?? false
  const lanListenEnabled = settings?.lanListenEnabled ?? false
  const controlsDisabled = !enabled || settingsBusy
  const lanFieldsDisabled = !enabled || !lanListenEnabled || settingsBusy

  // ── Body ─────────────────────────────────────────────────────────────
  return (
    <>
      <Sheet open={open} onOpenChange={onOpenChange}>
        <SheetContent side="right" className="w-full sm:max-w-md">
          <SheetHeader>
            <SheetTitle>{t('devices.mobileSync.settingsSheet.title')}</SheetTitle>
            <SheetDescription>{t('devices.mobileSync.settingsSheet.description')}</SheetDescription>
          </SheetHeader>

          <ScrollArea className="flex-1 px-4">
            <div className="space-y-3 py-2">
              {settings?.lanListenerError && (
                <Alert variant="destructive">
                  <AlertDescription>
                    {t('devices.mobileSync.statusBar.bindFailed', {
                      reason: settings.lanListenerError,
                    })}
                  </AlertDescription>
                </Alert>
              )}

              {settingsError && (
                <Alert variant="destructive">
                  <AlertDescription>{settingsError}</AlertDescription>
                </Alert>
              )}

              {/* ── 5 行 SettingsRow ───────────────────────────────── */}
              <div className="rounded-xl border border-border/60 bg-card divide-y divide-border/40">
                <SettingsRow
                  title={t('devices.mobileSync.enabled.label')}
                  description={t('devices.mobileSync.enabled.description')}
                  control={
                    <Switch
                      checked={enabled}
                      disabled={settingsBusy}
                      onCheckedChange={handleEnabledToggle}
                    />
                  }
                />

                <SettingsRow
                  title={t('devices.mobileSync.lanListener.label')}
                  description={t('devices.mobileSync.lanListener.description')}
                  disabled={controlsDisabled}
                  control={
                    <Switch
                      checked={lanListenEnabled}
                      disabled={controlsDisabled}
                      onCheckedChange={handleLanListenToggleRequest}
                    />
                  }
                />

                <SettingsRow
                  title={t('devices.mobileSync.lanListener.bindIp.label')}
                  disabled={lanFieldsDisabled}
                  control={
                    <Select
                      value={settings?.lanAdvertiseIp ?? BIND_IP_AUTO_SENTINEL}
                      disabled={lanFieldsDisabled}
                      onValueChange={handleBindIpChange}
                    >
                      <SelectTrigger className="w-[220px]">
                        <SelectValue
                          placeholder={t('devices.mobileSync.lanListener.bindIp.placeholder')}
                        />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={BIND_IP_AUTO_SENTINEL}>
                          {t('devices.mobileSync.lanListener.bindIp.auto')}
                        </SelectItem>
                        {lanInterfaces.length === 0 ? (
                          <div className="px-2 py-1.5 text-xs text-muted-foreground">
                            {t('devices.mobileSync.lanListener.bindIp.empty')}
                          </div>
                        ) : (
                          lanInterfaces.map(iface => (
                            <SelectItem key={`${iface.name}-${iface.ipv4}`} value={iface.ipv4}>
                              {iface.name} — {iface.ipv4}
                            </SelectItem>
                          ))
                        )}
                      </SelectContent>
                    </Select>
                  }
                />

                <SettingsRow
                  title={t('devices.mobileSync.lanListener.port.label')}
                  disabled={lanFieldsDisabled}
                  control={
                    <Input
                      type="number"
                      min={1}
                      max={65535}
                      inputMode="numeric"
                      className="w-[120px]"
                      disabled={lanFieldsDisabled}
                      placeholder={t('devices.mobileSync.lanListener.port.placeholder')}
                      value={portDraft}
                      onChange={e => setPortDraft(e.target.value)}
                      onBlur={handlePortBlur}
                    />
                  }
                />

                <SettingsRow
                  title={t('devices.mobileSync.lanListener.currentUrl.label')}
                  control={
                    <code className="rounded bg-muted px-2 py-1 font-mono text-xs">
                      {settings
                        ? deriveListenUrl(settings)
                        : t('devices.mobileSync.lanListener.currentUrl.unavailable')}
                    </code>
                  }
                />
              </div>
            </div>
          </ScrollArea>

          <SheetFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t('devices.mobileSync.settingsSheet.close')}
            </Button>
          </SheetFooter>
        </SheetContent>
      </Sheet>

      {/* ── LAN 安全告警 modal(嵌在 Sheet 外,Radix portal 自然堆叠在 Sheet 之上) */}
      <AlertDialog
        open={pendingLanEnable}
        onOpenChange={open => !open && setPendingLanEnable(false)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('devices.mobileSync.lanListener.warning.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileSync.lanListener.warning.body')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              {t('devices.mobileSync.lanListener.warning.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction onClick={handleLanWarningConfirm}>
              {t('devices.mobileSync.lanListener.warning.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

// ─── Subcomponents ─────────────────────────────────────────────────────

interface SettingsRowProps {
  title: string
  description?: string
  control: React.ReactNode
  disabled?: boolean
}

const SettingsRow: React.FC<SettingsRowProps> = ({ title, description, control, disabled }) => (
  <div
    className={`flex items-center justify-between gap-4 px-4 py-2.5 ${disabled ? 'opacity-60' : ''}`}
  >
    <div className="min-w-0 flex-1">
      <Label className="text-sm font-normal text-foreground">{title}</Label>
      {description && (
        <p className="mt-0.5 text-xs leading-snug text-muted-foreground">{description}</p>
      )}
    </div>
    <div className="shrink-0">{control}</div>
  </div>
)

// ─── Helpers ───────────────────────────────────────────────────────────

/**
 * 把 Tauri 抛出的错误翻译成用户可见文案。本组件实际触发的 settings/restart
 * 路径每条 i18n 都从这里走。其余 register 路径专属 variant 走兜底 unknown。
 */
function translateMobileSyncError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      case 'INVALID_LAN_PARAMETER':
        return t('devices.mobileSync.errors.invalidLanParameter', { reason: e.reason })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileSync.errors.settingsLoadFailed', { message: e.message })
      case 'SETTINGS_SAVE_FAILED':
        return t('devices.mobileSync.errors.settingsSaveFailed', { message: e.message })
      case 'ENDPOINT_INFO_PROBE_FAILED':
        return t('devices.mobileSync.errors.endpointInfoProbeFailed', { message: e.message })
      case 'LAN_PROBE_FAILED':
        return t('devices.mobileSync.errors.lanProbeFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
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

export default MobileSyncSettingsSheet
