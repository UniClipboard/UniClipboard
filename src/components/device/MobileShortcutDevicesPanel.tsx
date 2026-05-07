/**
 * MobileShortcutDevicesPanel —— DevicesPage 上 P2P SpaceMembersPanel 之外的
 * 第二个 panel,管理通过 SyncClipboard 快捷指令接入的 iPhone。
 *
 * 与 SpaceMembersPanel 的差异:
 * - 不进 Redux devicesSlice;直接调 Tauri command(GUI in-process facade
 *   第一例,见 src/api/tauri-command/mobile_sync.ts)
 * - 视觉上同级,但功能性质完全不同(明文 LAN HTTP / 单向请求 / Basic Auth /
 *   仅本机 LAN)
 *
 * 本文件 Step 4 范围:总开关 / LAN 监听 / IP 选择 / 安全告警 modal /
 * 当前 LAN URL 显示 / 已配对设备列表 / revoke 确认。
 * **不包含**「添加 iPhone」对话框与「凭据展示」modal —— 留给 Step 5。
 * 「添加」按钮当前是 stub(打开占位 toast);Step 5 接上后即可端到端。
 */

import { Plus, RefreshCw, Smartphone, Trash2 } from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import AddMobileShortcutDeviceDialog from './AddMobileShortcutDeviceDialog'
import MobileShortcutCredentialModal from './MobileShortcutCredentialModal'
import {
  getMobileSyncSettings,
  isMobileSyncError,
  listMobileDevices,
  listMobileLanInterfaces,
  revokeMobileDevice,
  updateMobileSyncSettings,
  type LanInterfaceView,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RegisterMobileDeviceResult,
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-shortcut-panel')

const MobileShortcutDevicesPanel: React.FC = () => {
  const { t } = useTranslation()

  // ── State ────────────────────────────────────────────────────────────
  const [settings, setSettings] = useState<MobileSyncSettingsView | null>(null)
  const [settingsError, setSettingsError] = useState<string | null>(null)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const [restartRequired, setRestartRequired] = useState(false)
  const [restartDismissed, setRestartDismissed] = useState(false)

  const [devices, setDevices] = useState<MobileDeviceView[]>([])
  const [devicesError, setDevicesError] = useState<string | null>(null)

  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])

  // 待确认的"开启 LAN 监听"动作 —— 弹安全告警 modal,确认后才真改 settings
  const [pendingLanEnable, setPendingLanEnable] = useState(false)

  // 待撤销的设备 —— 选中后弹 AlertDialog 二次确认
  const [revokeTarget, setRevokeTarget] = useState<MobileDeviceView | null>(null)
  const [revokeBusy, setRevokeBusy] = useState(false)

  // 端口输入框的本地草稿 —— blur 时才提交,避免每次 keystroke 都打 facade
  const [portDraft, setPortDraft] = useState<string>('')

  // Add iPhone dialog 开关
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  // 注册成功后的一次性凭据 payload —— null 表示 modal 关闭。本组件持有
  // 时间窗口仅"用户尚未勾选已保存"那一刻;关闭时显式清空,避免在内存
  // 长期保留明文密码。
  const [credentialPayload, setCredentialPayload] = useState<RegisterMobileDeviceResult | null>(
    null
  )

  // ── Helpers ──────────────────────────────────────────────────────────
  const translate = useCallback((err: unknown): string => translateMobileSyncError(t, err), [t])

  // ── Loaders ──────────────────────────────────────────────────────────
  const loadSettings = useCallback(async () => {
    try {
      const view = await getMobileSyncSettings()
      setSettings(view)
      setSettingsError(null)
      setPortDraft(view.lanPort != null ? String(view.lanPort) : '')
    } catch (err) {
      log.error({ err }, 'failed to load mobile sync settings')
      setSettingsError(translate(err))
    }
  }, [translate])

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

  const loadLanInterfaces = useCallback(async () => {
    try {
      const list = await listMobileLanInterfaces()
      setLanInterfaces(list)
    } catch (err) {
      // 列接口失败不阻塞主面板;只 toast,Select 会显示空态
      log.warn({ err }, 'failed to list LAN interfaces')
      toast.error(translate(err))
    }
  }, [translate])

  useEffect(() => {
    void loadSettings()
    void loadDevices()
    void loadLanInterfaces()
  }, [loadSettings, loadDevices, loadLanInterfaces])

  // ── Mutators ─────────────────────────────────────────────────────────
  const applySettingsUpdate = useCallback(
    async (patch: Parameters<typeof updateMobileSyncSettings>[0], onSuccessMessage?: string) => {
      setSettingsBusy(true)
      try {
        const result = await updateMobileSyncSettings(patch)
        setSettings(prev =>
          prev
            ? {
                ...prev,
                enabled: result.enabled,
                lanListenEnabled: result.lanListenEnabled,
                lanAdvertiseIp: result.lanAdvertiseIp,
                lanPort: result.lanPort,
              }
            : prev
        )
        setPortDraft(result.lanPort != null ? String(result.lanPort) : '')
        if (result.restartRequired) {
          setRestartRequired(true)
          setRestartDismissed(false)
        }
        if (onSuccessMessage) toast.success(onSuccessMessage)
        // 设置变更后 currentLanUrl 由 daemon 探测决定,需要重新拉一次 view
        await loadSettings()
      } catch (err) {
        log.error({ err, patch }, 'failed to update mobile sync settings')
        toast.error(translate(err))
      } finally {
        setSettingsBusy(false)
      }
    },
    [loadSettings, translate]
  )

  const handleEnabledToggle = useCallback(
    (next: boolean) => {
      void applySettingsUpdate({ enabled: next })
    },
    [applySettingsUpdate]
  )

  const handleLanListenToggleRequest = useCallback(
    (next: boolean) => {
      // 关闭直接执行;开启需要弹安全告警让用户主动确认
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
      // Select 不能传 null,本组件用空串约定为"清空(用 daemon 默认)"
      void applySettingsUpdate({ lanAdvertiseIp: value === '' ? null : value })
    },
    [applySettingsUpdate]
  )

  const handlePortBlur = useCallback(() => {
    if (!settings) return
    const trimmed = portDraft.trim()
    if (trimmed === '') {
      // 清空 → 用 daemon 默认(42720)
      if (settings.lanPort != null) {
        void applySettingsUpdate({ lanPort: null })
      }
      return
    }
    const parsed = Number(trimmed)
    if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) {
      // 非法输入 → 回滚草稿,不打 facade
      setPortDraft(settings.lanPort != null ? String(settings.lanPort) : '')
      toast.error(
        t('devices.mobileShortcut.errors.invalidLanParameter', {
          reason: t('devices.mobileShortcut.lanListener.port.label'),
        })
      )
      return
    }
    if (parsed !== settings.lanPort) {
      void applySettingsUpdate({ lanPort: parsed })
    }
  }, [applySettingsUpdate, portDraft, settings, t])

  const handleRestart = useCallback(async () => {
    try {
      const { invokeWithTrace } = await import('@/lib/tauri-command')
      await invokeWithTrace('restart_app')
    } catch (err) {
      log.error({ err }, 'failed to restart')
      toast.error(translate(err))
    }
  }, [translate])

  const handleOpenAddDialog = useCallback(() => {
    if (!settings?.lanListenEnabled) {
      // 兜底:按钮 disabled 已挡,这里只是防御(避免直接 setState 时 race)
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

  const handleCredentialClose = useCallback(() => {
    setCredentialPayload(null)
  }, [])

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
  const controlsDisabled = !enabled || settingsBusy
  const lanFieldsDisabled = !enabled || !lanListenEnabled || settingsBusy

  // ── Body ─────────────────────────────────────────────────────────────
  if (settingsError) {
    return (
      <section className="space-y-2">
        <PanelHeader t={t} />
        <Alert variant="destructive">
          <AlertDescription className="flex items-center gap-3">
            <span className="flex-1">{settingsError}</span>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => void loadSettings()}
              title={t('devices.list.actions.retry')}
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
          </AlertDescription>
        </Alert>
      </section>
    )
  }

  if (!settings) {
    // 首次加载占位 —— 主面板从未拿到 settings,显式空标题让用户知道"在加载"
    return (
      <section className="space-y-2">
        <PanelHeader t={t} />
        <div className="rounded-xl border border-border/60 bg-card p-6 text-center text-sm text-muted-foreground">
          {t('devices.mobileShortcut.errors.loadFailed', { message: '...' })}
        </div>
      </section>
    )
  }

  return (
    <>
      <section className="space-y-4">
        <PanelHeader t={t} />

        {/* ── 设置卡片 ───────────────────────────────────────── */}
        <div className="rounded-xl border border-border/60 bg-card divide-y divide-border/40">
          {/* 总开关 */}
          <SettingsRow
            title={t('devices.mobileShortcut.enabled.label')}
            description={t('devices.mobileShortcut.enabled.description')}
            control={
              <Switch
                checked={enabled}
                disabled={settingsBusy}
                onCheckedChange={handleEnabledToggle}
              />
            }
          />

          {/* LAN 监听 */}
          <SettingsRow
            title={t('devices.mobileShortcut.lanListener.label')}
            description={t('devices.mobileShortcut.lanListener.description')}
            disabled={controlsDisabled}
            control={
              <Switch
                checked={lanListenEnabled}
                disabled={controlsDisabled}
                onCheckedChange={handleLanListenToggleRequest}
              />
            }
          />

          {/* Bind IP */}
          <SettingsRow
            title={t('devices.mobileShortcut.lanListener.bindIp.label')}
            disabled={lanFieldsDisabled}
            control={
              <Select
                value={settings.lanAdvertiseIp ?? ''}
                disabled={lanFieldsDisabled}
                onValueChange={handleBindIpChange}
              >
                <SelectTrigger className="w-[260px]">
                  <SelectValue
                    placeholder={t('devices.mobileShortcut.lanListener.bindIp.placeholder')}
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="">
                    {t('devices.mobileShortcut.lanListener.bindIp.auto')}
                  </SelectItem>
                  {lanInterfaces.length === 0 ? (
                    <div className="px-2 py-1.5 text-xs text-muted-foreground">
                      {t('devices.mobileShortcut.lanListener.bindIp.empty')}
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

          {/* Port */}
          <SettingsRow
            title={t('devices.mobileShortcut.lanListener.port.label')}
            disabled={lanFieldsDisabled}
            control={
              <Input
                type="number"
                min={1}
                max={65535}
                inputMode="numeric"
                className="w-[120px]"
                disabled={lanFieldsDisabled}
                placeholder={t('devices.mobileShortcut.lanListener.port.placeholder')}
                value={portDraft}
                onChange={e => setPortDraft(e.target.value)}
                onBlur={handlePortBlur}
              />
            }
          />

          {/* 当前 LAN URL */}
          <SettingsRow
            title={t('devices.mobileShortcut.lanListener.currentUrl.label')}
            control={
              <code className="rounded bg-muted px-2 py-1 font-mono text-xs">
                {settings.currentLanUrl ??
                  t('devices.mobileShortcut.lanListener.currentUrl.unavailable')}
              </code>
            }
          />
        </div>

        {/* ── 重启提示 ───────────────────────────────────────── */}
        {restartRequired && !restartDismissed && (
          <Alert className="border-amber-500/30 bg-amber-500/10">
            <AlertDescription className="flex items-center gap-3 text-amber-700 dark:text-amber-400">
              <span className="flex-1">
                {t('devices.mobileShortcut.lanListener.restartRequired.message')}
              </span>
              <Button size="sm" variant="outline" onClick={handleRestart}>
                {t('devices.mobileShortcut.lanListener.restartRequired.restartButton')}
              </Button>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={t(
                  'devices.mobileShortcut.lanListener.restartRequired.dismissAriaLabel'
                )}
                onClick={() => setRestartDismissed(true)}
              >
                ×
              </Button>
            </AlertDescription>
          </Alert>
        )}

        {/* ── 设备列表 ───────────────────────────────────────── */}
        <div className="space-y-2">
          <div className="flex items-center justify-between px-1">
            <h4 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
              {t('devices.mobileShortcut.list.title')}
            </h4>
            {devicesError && (
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => void loadDevices()}
                title={t('devices.list.actions.retry')}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            )}
          </div>

          {devicesError ? (
            <Alert variant="destructive">
              <AlertDescription>{devicesError}</AlertDescription>
            </Alert>
          ) : devices.length === 0 ? (
            <EmptyState t={t} onAdd={handleOpenAddDialog} disabled={!lanListenEnabled} />
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {devices.map(device => (
                <DeviceCard
                  key={device.deviceId}
                  device={device}
                  onRevoke={() => setRevokeTarget(device)}
                />
              ))}
              <AddDeviceCard t={t} onAdd={handleOpenAddDialog} disabled={!lanListenEnabled} />
            </div>
          )}

          {!lanListenEnabled && (
            <p className="px-1 text-xs text-muted-foreground">
              {t('devices.mobileShortcut.lanListener.disabledHint')}
            </p>
          )}
        </div>
      </section>

      {/* ── LAN 安全告警 modal ──────────────────────────────── */}
      <AlertDialog
        open={pendingLanEnable}
        onOpenChange={open => !open && setPendingLanEnable(false)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('devices.mobileShortcut.lanListener.warning.title')}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileShortcut.lanListener.warning.body')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              {t('devices.mobileShortcut.lanListener.warning.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction onClick={handleLanWarningConfirm}>
              {t('devices.mobileShortcut.lanListener.warning.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* ── Revoke 确认 modal ───────────────────────────────── */}
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

      {/* ── Add iPhone 表单 dialog ──────────────────────────── */}
      <AddMobileShortcutDeviceDialog
        open={addDialogOpen}
        onOpenChange={setAddDialogOpen}
        onSuccess={handleAddSuccess}
      />

      {/* ── 一次性凭据展示 modal ────────────────────────────── */}
      <MobileShortcutCredentialModal payload={credentialPayload} onClose={handleCredentialClose} />
    </>
  )
}

// ─── Subcomponents ─────────────────────────────────────────────────────

const PanelHeader: React.FC<{ t: ReturnType<typeof useTranslation>['t'] }> = ({ t }) => (
  <div className="px-1">
    <div className="flex items-center gap-2">
      <Smartphone className="h-4 w-4 text-muted-foreground" />
      <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {t('devices.mobileShortcut.title')}
      </h3>
    </div>
    <p className="mt-1 text-xs text-muted-foreground/80">
      {t('devices.mobileShortcut.description')}
    </p>
  </div>
)

interface SettingsRowProps {
  title: string
  description?: string
  control: React.ReactNode
  disabled?: boolean
}

const SettingsRow: React.FC<SettingsRowProps> = ({ title, description, control, disabled }) => (
  <div
    className={`flex items-center justify-between gap-4 px-4 py-3 ${disabled ? 'opacity-60' : ''}`}
  >
    <div className="min-w-0 flex-1">
      <Label className="text-sm font-medium">{title}</Label>
      {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
    </div>
    <div className="shrink-0">{control}</div>
  </div>
)

interface DeviceCardProps {
  device: MobileDeviceView
  onRevoke: () => void
}

const DeviceCard: React.FC<DeviceCardProps> = ({ device, onRevoke }) => {
  const { t } = useTranslation()
  const lastSeenLabel = useMemo(() => {
    if (device.lastSeenAtMs == null) {
      return t('devices.mobileShortcut.list.lastSeen.never')
    }
    return t('devices.mobileShortcut.list.lastSeen.format', {
      relative: formatRelativeMs(device.lastSeenAtMs),
    })
  }, [device.lastSeenAtMs, t])

  return (
    <div className="group relative flex flex-col items-center rounded-2xl bg-card p-5 pt-6 pb-4 text-center shadow-sm ring-1 ring-border/40 transition-all hover:shadow-md hover:ring-border/60">
      <div className="mb-3 h-14 w-14 rounded-2xl flex items-center justify-center bg-blue-500/10 text-blue-600 dark:text-blue-400">
        <Smartphone className="h-7 w-7" />
      </div>
      <span className="truncate w-full font-medium text-foreground text-sm leading-tight">
        {device.label}
      </span>
      <span className="mt-1 text-xs text-muted-foreground">{lastSeenLabel}</span>
      {device.lastSeenIp && (
        <span className="mt-0.5 text-xs text-muted-foreground/70">
          {t('devices.mobileShortcut.list.lastSeen.fromIp', { ip: device.lastSeenIp })}
        </span>
      )}
      <Button
        variant="ghost"
        size="icon-sm"
        className="absolute right-2 top-2 opacity-0 transition-opacity group-hover:opacity-100"
        onClick={onRevoke}
        aria-label={t('devices.mobileShortcut.revoke.confirm')}
        title={t('devices.mobileShortcut.revoke.confirm')}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </Button>
    </div>
  )
}

interface EmptyStateProps {
  t: ReturnType<typeof useTranslation>['t']
  onAdd: () => void
  disabled: boolean
}

const EmptyState: React.FC<EmptyStateProps> = ({ t, onAdd, disabled }) => (
  <div className="flex flex-col items-center rounded-xl border border-dashed border-border/80 py-10 px-6 text-center">
    <div className="mb-4 rounded-full bg-muted/50 p-4 ring-1 ring-border/50">
      <Smartphone className="h-8 w-8 text-muted-foreground/70" />
    </div>
    <h3 className="mb-1.5 text-sm font-medium text-foreground">
      {t('devices.mobileShortcut.list.empty.title')}
    </h3>
    <p className="mb-4 max-w-xs text-xs text-muted-foreground">
      {t('devices.mobileShortcut.list.empty.description')}
    </p>
    <Button variant="outline" size="sm" onClick={onAdd} disabled={disabled}>
      <Plus className="h-3.5 w-3.5" />
      {t('devices.mobileShortcut.list.addButton')}
    </Button>
  </div>
)

interface AddDeviceCardProps {
  t: ReturnType<typeof useTranslation>['t']
  onAdd: () => void
  disabled: boolean
}

const AddDeviceCard: React.FC<AddDeviceCardProps> = ({ t, onAdd, disabled }) => (
  <button
    type="button"
    onClick={onAdd}
    disabled={disabled}
    className="group flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-border/60 p-5 pt-6 pb-4 text-center transition-all hover:border-border hover:bg-muted/30 cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
  >
    <div className="mb-3 h-14 w-14 rounded-2xl flex items-center justify-center bg-muted/50 transition-colors group-hover:bg-muted">
      <Plus className="h-7 w-7 text-muted-foreground/70 group-hover:text-foreground" />
    </div>
    <span className="text-sm font-medium text-muted-foreground group-hover:text-foreground">
      {t('devices.mobileShortcut.list.addButton')}
    </span>
  </button>
)

// ─── Helpers ───────────────────────────────────────────────────────────

/** 把 last-seen 毫秒时间戳格成 "5m" / "2h" / "3d" 形态(与 clipboard-utils 风格一致)。 */
function formatRelativeMs(timestampMs: number): string {
  const diffMs = Date.now() - timestampMs
  const diffMins = Math.round(diffMs / 60_000)
  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m`
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h`
  return `${Math.floor(diffMins / 1440)}d`
}

/**
 * 把 Tauri 抛出的错误翻译成用户可见文案。`MobileSyncError` discriminated
 * union 走专门的 i18n key + 结构化 payload(min/max/username 等);其余非
 * 我们的错误兜底为 `errors.unknown`。
 */
function translateMobileSyncError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileShortcut.errors.facadeUnavailable')
      case 'LABEL_EMPTY':
        return t('devices.mobileShortcut.errors.labelEmpty')
      case 'LABEL_TOO_LONG':
        return t('devices.mobileShortcut.errors.labelTooLong', { max: e.max })
      case 'LAN_LISTENER_DISABLED':
        return t('devices.mobileShortcut.errors.lanListenerDisabled')
      case 'USERNAME_TAKEN':
        return t('devices.mobileShortcut.errors.usernameTaken', { username: e.username })
      case 'USERNAME_INVALID_SHAPE':
        return t('devices.mobileShortcut.errors.usernameInvalidShape', { reason: e.reason })
      case 'PASSWORD_TOO_SHORT':
        return t('devices.mobileShortcut.errors.passwordTooShort', { min: e.min })
      case 'PASSWORD_TOO_LONG':
        return t('devices.mobileShortcut.errors.passwordTooLong', { max: e.max })
      case 'PASSWORD_HASH_FAILED':
        return t('devices.mobileShortcut.errors.passwordHashFailed', { message: e.message })
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileShortcut.errors.deviceNotFound')
      case 'INVALID_LAN_PARAMETER':
        return t('devices.mobileShortcut.errors.invalidLanParameter', { reason: e.reason })
      case 'SETTINGS_LOAD_FAILED':
        return t('devices.mobileShortcut.errors.settingsLoadFailed', { message: e.message })
      case 'SETTINGS_SAVE_FAILED':
        return t('devices.mobileShortcut.errors.settingsSaveFailed', { message: e.message })
      case 'ENDPOINT_INFO_PROBE_FAILED':
        return t('devices.mobileShortcut.errors.endpointInfoProbeFailed', { message: e.message })
      case 'LAN_PROBE_FAILED':
        return t('devices.mobileShortcut.errors.lanProbeFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileShortcut.errors.persistenceFailed', { message: e.message })
      case 'QR_RENDER_FAILED':
        return t('devices.mobileShortcut.errors.qrRenderFailed', { message: e.message })
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileShortcut.errors.unknown', { message })
}

// 便于 Step 5 测试 / 复用的 helper 导出（不是 panel 公共 API）
export const __test__ = { translateMobileSyncError, formatRelativeMs }

export default MobileShortcutDevicesPanel
