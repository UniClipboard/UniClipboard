/**
 * MobileSyncSettingsDialog —— 移动设备同步全局配置 modal。
 *
 * 视觉语言严格遵循 V2 原型的 modal 设计（与 [[DeviceSettingsDialog]] 同源）：
 *
 *   ┌────────────────────────────────────────────────────────────┐
 *   │ 📱 移动设备同步配置                                          │
 *   │    调整移动设备通过局域网访问本机所需的开关与网络参数        │
 *   ├─ alerts (lanListenerError / settingsError) ────────────────┤
 *   ├─ 同步开关 ─────────────────────────────────────────────────┤
 *   │ ┌─ 启用移动端同步 ───────────────────────────────  [●] ┐   │
 *   │ │   关闭后将立即拒绝来自所有移动设备的同步请求           │   │
 *   │ └────────────────────────────────────────────────────┘   │
 *   │ ┌─ LAN 监听 ──────────────────────────────────────  [●] ┐  │
 *   │ │   在所选网络接口上监听明文 HTTP，开启后才能接收请求    │  │
 *   │ └────────────────────────────────────────────────────┘   │
 *   ├─ 网络参数 ─────────────────────────────────────────────────┤
 *   │ ┌─ 监听 IP                                     [Select] ┐  │
 *   │ └────────────────────────────────────────────────────┘   │
 *   │ ┌─ 端口                                        [42720]  ┐  │
 *   │ └────────────────────────────────────────────────────┘   │
 *   │ ┌─ 当前监听地址         http://192.168.1.42:42720 [复制] ┐  │
 *   │ └────────────────────────────────────────────────────┘   │
 *   ├────────────────────────────────────────────────────────────┤
 *   │                                                    [完成] │
 *   └────────────────────────────────────────────────────────────┘
 *
 * 区块辅助组件（DialogSection / ListenUrlInfoRow / SettingControlRow / SettingToggleRow）
 * 与 DeviceSettingsDialog 同形：圆角 border bg-card/50，title `[11px]
 * uppercase tracking-wider`，控件靠右。
 *
 * # 关键不变量
 *
 * - LAN 安全告警：开启 LAN 监听仍弹 AlertDialog（Radix portal 自然堆叠在
 *   Dialog 之上）
 * - port: 本地 portDraft + onBlur 提交，避免每键击都触发 update_settings
 * - bindIp: BIND_IP_AUTO_SENTINEL ↔ null 互转（Auto 选项）
 * - applySettingsUpdate 成功 toast.success(applied)，bind 失败 toast.error
 *   + 透传 reason（result.lanListenerBindError）
 * - onSettingsChange 回调把最新 settings 视图回传给父 panel
 */

import { Check, Copy, Smartphone } from 'lucide-react'
import React, { useCallback, useEffect, useRef, useState } from 'react'
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-sync-settings-dialog')

/**
 * Radix Select 禁止 SelectItem.value 为空串；用非空 sentinel 表示"自动"，
 * 在 boundary 处与 facade 的 null 互转。
 */
const BIND_IP_AUTO_SENTINEL = '__auto__'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /**
   * 把最新 settings 视图回调给父 panel。null 表示加载失败 / 首次未就绪。
   * 父 panel 用它驱动状态条文案 + Add 按钮 disabled。
   */
  onSettingsChange?: (settings: MobileSyncSettingsView | null) => void
}

const MobileSyncSettingsDialog: React.FC<Props> = ({ open, onOpenChange, onSettingsChange }) => {
  const { t } = useTranslation()

  // ── State ────────────────────────────────────────────────────────────
  const [settings, setSettings] = useState<MobileSyncSettingsView | null>(null)
  const [settingsError, setSettingsError] = useState<string | null>(null)
  const [settingsBusy, setSettingsBusy] = useState(false)
  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])
  const [pendingLanEnable, setPendingLanEnable] = useState(false)
  const [portDraft, setPortDraft] = useState<string>('')

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

  // 首次挂载就拉一次，状态条立刻有数据；Dialog 关闭后下次打开不重新 mount，
  // 所以只跑一次。
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
        // 落盘成功 ≠ listener 起来；bind 失败时 facade 透传 reason 到
        // lanListenerBindError，这里 toast.error 让用户立刻知道。
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
        // lanListenerError 等运行时字段由 daemon 写入，update 返回值只含持久化
        // 字段；需 reload 拿最新视图。
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
  const lanListenDisabled = !enabled || settingsBusy
  const lanFieldsDisabled = !enabled || !lanListenEnabled || settingsBusy

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <div className="flex items-center gap-3">
              <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-blue-500/10 text-blue-600 dark:text-blue-400">
                <Smartphone className="h-5 w-5" />
              </div>
              <div className="min-w-0 flex-1">
                <DialogTitle className="truncate text-left">
                  {t('devices.mobileSync.settingsSheet.title')}
                </DialogTitle>
                <DialogDescription className="mt-1 text-left text-xs leading-snug">
                  {t('devices.mobileSync.settingsSheet.description')}
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>

          <div className="space-y-5">
            {(settings?.lanListenerError || settingsError) && (
              <div className="space-y-2">
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
              </div>
            )}

            {/* ── 同步开关 ─────────────────────────────────────── */}
            <DialogSection
              title={t('devices.mobileSync.sections.switches', { defaultValue: '同步开关' })}
            >
              <SettingToggleRow
                label={t('devices.mobileSync.enabled.label')}
                description={t('devices.mobileSync.enabled.description')}
                checked={enabled}
                disabled={settingsBusy}
                onChange={handleEnabledToggle}
              />
              <SettingToggleRow
                label={t('devices.mobileSync.lanListener.label')}
                description={t('devices.mobileSync.lanListener.description')}
                checked={lanListenEnabled}
                disabled={lanListenDisabled}
                onChange={handleLanListenToggleRequest}
              />
            </DialogSection>

            {/* ── 网络参数 ─────────────────────────────────────── */}
            <DialogSection
              title={t('devices.mobileSync.sections.network', { defaultValue: '网络参数' })}
            >
              <SettingControlRow
                label={t('devices.mobileSync.lanListener.bindIp.label')}
                disabled={lanFieldsDisabled}
                control={
                  <Select
                    value={settings?.lanAdvertiseIp ?? BIND_IP_AUTO_SENTINEL}
                    disabled={lanFieldsDisabled}
                    onValueChange={handleBindIpChange}
                  >
                    <SelectTrigger size="sm" className="w-[180px]">
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
              <SettingControlRow
                label={t('devices.mobileSync.lanListener.port.label')}
                disabled={lanFieldsDisabled}
                control={
                  <Input
                    type="number"
                    min={1}
                    max={65535}
                    inputMode="numeric"
                    className="h-8 w-[100px]"
                    disabled={lanFieldsDisabled}
                    placeholder={t('devices.mobileSync.lanListener.port.placeholder')}
                    value={portDraft}
                    onChange={e => setPortDraft(e.target.value)}
                    onBlur={handlePortBlur}
                  />
                }
              />
              <ListenUrlInfoRow
                label={t('devices.mobileSync.lanListener.currentUrl.label')}
                settings={settings}
                lanInterfaces={lanInterfaces}
                lanListenEnabled={lanListenEnabled}
                unavailableLabel={t('devices.mobileSync.lanListener.currentUrl.unavailable')}
              />
            </DialogSection>
          </div>

          <DialogFooter className="!flex-row !justify-end">
            <Button size="sm" onClick={() => onOpenChange(false)}>
              {t('devices.mobileSync.settingsSheet.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── LAN 安全告警 modal（Radix portal 自然堆叠在主 Dialog 之上） */}
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

export default MobileSyncSettingsDialog

// ────────────────────────────────────────────────────────────────
// Section / row helpers (本文件局部)
// ────────────────────────────────────────────────────────────────

const DialogSection: React.FC<{
  title: string
  trailing?: React.ReactNode
  children: React.ReactNode
}> = ({ title, trailing, children }) => (
  <section className="space-y-2">
    <div className="flex items-center justify-between px-1">
      <h5 className="text-[11px] uppercase tracking-wider text-muted-foreground">{title}</h5>
      {trailing}
    </div>
    <div className="space-y-2">{children}</div>
  </section>
)

/**
 * 当前监听地址行的渲染分四档:
 *
 * 1. settings 未加载 / LAN 监听未开 → 单行 unavailable("—")
 * 2. lanAdvertiseIp 显式指定 → 单行 URL 卡片 + 复制按钮
 * 3. Auto + 至少一个 LAN 接口 → 整行变 vertical block,内联列出全部候选
 *    IP,逐个可复制。daemon 永远 bind 0.0.0.0,客户端必须拿到真实可达的
 *    LAN 地址 —— Auto 时哪一个能通要看客户端所处的网段,所以让用户在
 *    多个候选中自己选,而不是替他猜一个。
 * 4. Auto + 无可用 LAN 接口 → 单行 unavailable("—")
 *
 * 候选 IP 的来源 / 顺序与 daemon `auto_pick_advertise_ip` 完全一致
 * (`register_device.rs:207-208` 声明两处口径绑定),所以本组件不再复制
 * 排序策略,直接消费 listMobileLanInterfaces 的结果即可。
 */
const ListenUrlInfoRow: React.FC<{
  label: string
  settings: MobileSyncSettingsView | null
  lanInterfaces: LanInterfaceView[]
  lanListenEnabled: boolean
  unavailableLabel: string
}> = ({ label, settings, lanInterfaces, lanListenEnabled, unavailableLabel }) => {
  // Auto + 有候选 → 独立 block 布局(label + hint + 列表)
  if (settings && lanListenEnabled && !settings.lanAdvertiseIp && lanInterfaces.length > 0) {
    return (
      <AutoListenUrlBlock
        label={label}
        interfaces={lanInterfaces}
        port={settings.lanPort ?? 42720}
      />
    )
  }

  // 其它三档共用单行 flex 布局
  let content: React.ReactNode
  if (!settings || !lanListenEnabled) {
    content = <UnavailableUrl label={unavailableLabel} />
  } else if (settings.lanAdvertiseIp) {
    content = <ListenUrlControl url={deriveListenUrl(settings)} />
  } else {
    // Auto + 无候选
    content = <UnavailableUrl label={unavailableLabel} />
  }

  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2 text-xs">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      {content}
    </div>
  )
}

const UnavailableUrl: React.FC<{ label: string }> = ({ label }) => (
  <span className="font-mono text-foreground">{label}</span>
)

/**
 * "复制后 1.5s 还原 Check 图标"的反馈节奏,抽出来给单 URL / Auto popover
 * 两条路径共享。timer 用 ref 持有,unmount 与下次点击都清理一次,避免在
 * 已卸载组件上 setState 或多次点击叠加 timer。
 */
function useCopyWithFeedback(): {
  copied: boolean
  copy: (url: string) => Promise<void>
} {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    },
    []
  )

  const copy = useCallback(
    async (url: string) => {
      try {
        await navigator.clipboard.writeText(url)
        setCopied(true)
        if (timerRef.current) clearTimeout(timerRef.current)
        timerRef.current = setTimeout(() => setCopied(false), 1500)
      } catch (err) {
        log.warn({ err }, 'failed to copy listen url')
        toast.error(t('clipboard.errors.copyFailed'))
      }
    },
    [t]
  )

  return { copied, copy }
}

const ListenUrlControl: React.FC<{ url: string }> = ({ url }) => {
  const { t } = useTranslation()
  const { copied, copy } = useCopyWithFeedback()

  const copyLabel = copied
    ? t('devices.mobileSync.lanListener.currentUrl.copied')
    : t('devices.mobileSync.lanListener.currentUrl.copy')

  return (
    <div className="flex min-w-0 max-w-56 items-center gap-1 sm:max-w-xs">
      <code
        className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1 font-mono text-xs text-foreground"
        title={url}
      >
        {url}
      </code>
      <Button
        type="button"
        size="icon-sm"
        variant="ghost"
        className="shrink-0"
        aria-label={copyLabel}
        title={copyLabel}
        onClick={() => void copy(url)}
      >
        {copied ? (
          <Check className="h-3.5 w-3.5 text-emerald-500" />
        ) : (
          <Copy className="h-3.5 w-3.5" />
        )}
      </Button>
    </div>
  )
}

/**
 * Auto 模式下的"当前监听地址" block:整行变 vertical stack,顶部一行 label +
 * "Auto" 角标,下方一行小字说明,再下方逐个 LAN 候选 IP,每条独立复制。
 *
 * 每行的"复制反馈"用 copiedIp 跟踪,而不是复用 useCopyWithFeedback 的单一
 * copied —— 用户可能依次复制多条对比,需要按行独立显示 Check。
 */
const AutoListenUrlBlock: React.FC<{
  label: string
  interfaces: LanInterfaceView[]
  port: number
}> = ({ label, interfaces, port }) => {
  const { t } = useTranslation()
  const [copiedIp, setCopiedIp] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    },
    []
  )

  const handleCopyOne = useCallback(
    async (url: string, ip: string) => {
      try {
        await navigator.clipboard.writeText(url)
        setCopiedIp(ip)
        if (timerRef.current) clearTimeout(timerRef.current)
        timerRef.current = setTimeout(() => setCopiedIp(null), 1500)
      } catch (err) {
        log.warn({ err }, 'failed to copy listen url')
        toast.error(t('clipboard.errors.copyFailed'))
      }
    },
    [t]
  )

  const autoLabel = t('devices.mobileSync.lanListener.currentUrl.auto.label')
  const hint = t('devices.mobileSync.lanListener.currentUrl.auto.hint')

  return (
    <div className="space-y-2 rounded-lg border border-border/60 bg-card/50 px-3 py-2 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="text-muted-foreground">{label}</span>
        <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
          {autoLabel}
        </span>
      </div>
      <p className="text-[11px] leading-snug text-muted-foreground">{hint}</p>
      <ul className="space-y-1">
        {interfaces.map(iface => {
          const url = `http://${iface.ipv4}:${port}`
          const isCopied = copiedIp === iface.ipv4
          const copyLabel = isCopied
            ? t('devices.mobileSync.lanListener.currentUrl.copied')
            : t('devices.mobileSync.lanListener.currentUrl.copy')
          return (
            <li
              key={`${iface.name}-${iface.ipv4}`}
              className="flex items-center gap-2 rounded-md border border-border/40 bg-background/60 px-2 py-1.5"
            >
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-xs text-foreground" title={url}>
                  {url}
                </p>
                <p className="text-[10px] leading-snug text-muted-foreground">{iface.name}</p>
              </div>
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                className="shrink-0"
                aria-label={copyLabel}
                title={copyLabel}
                onClick={() => void handleCopyOne(url, iface.ipv4)}
              >
                {isCopied ? (
                  <Check className="h-3.5 w-3.5 text-emerald-500" />
                ) : (
                  <Copy className="h-3.5 w-3.5" />
                )}
              </Button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}

const SettingToggleRow: React.FC<{
  label: string
  description?: string
  checked: boolean
  disabled?: boolean
  onChange: (v: boolean) => void
}> = ({ label, description, checked, disabled, onChange }) => (
  <div
    className={cn(
      'flex items-start justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2.5',
      disabled && 'opacity-60'
    )}
  >
    <div className="min-w-0 flex-1">
      <p className="text-sm font-medium text-foreground">{label}</p>
      {description && (
        <p className="mt-0.5 text-[11px] leading-snug text-muted-foreground">{description}</p>
      )}
    </div>
    <Switch checked={checked} onCheckedChange={onChange} disabled={disabled} />
  </div>
)

const SettingControlRow: React.FC<{
  label: string
  control: React.ReactNode
  disabled?: boolean
}> = ({ label, control, disabled }) => (
  <div
    className={cn(
      'flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2',
      disabled && 'opacity-60'
    )}
  >
    <span className="shrink-0 text-xs text-muted-foreground">{label}</span>
    <div className="shrink-0">{control}</div>
  </div>
)

// ────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────

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
      case 'ENDPOINT_INFO_FAILED':
        return t('devices.mobileSync.errors.endpointInfoFailed', { message: e.message })
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
