/**
 * MobileSyncDeviceDialog —— 已注册移动设备的统一管理 modal。
 *
 * 触发: DevicesPage 上 MobileCard 整卡点击。
 *
 * # 单 dialog 多视图设计 (替换了原先三个独立 modal)
 *
 *   ┌─ view = 'info' ───────────────────────────────────────────────┐
 *   │ 📱 label                                                       │
 *   │    username (mono)                                             │
 *   ├─ 服务地址 ─────────────────────────────────────────────────────┤
 *   │ baseUrl chip [▼]    [复制]                                    │
 *   │ ┌─────────────┐                                                │
 *   │ │ 大 QR / 占位 │  ← rotateResult 存在 → connect URI QR        │
 *   │ │             │     否则 → 灰色占位 "未持有明文,点改密生成"   │
 *   │ └─────────────┘                                                │
 *   ├─ 新密码 (仅 rotateResult 时)─────────────────────────────────┤
 *   │ ⚠ 旧密码失效, username / password / [备份]                    │
 *   │ ☐ 我已保存(未勾选则拦截关闭)                                  │
 *   ├─ 设备信息 ─────────────────────────────────────────────────────┤
 *   │ 添加时间 / 最近活动 / 最近来源 IP / 客户端名称 / 客户端系统     │
 *   ├────────────────────────────────────────────────────────────────┤
 *   │ [修改密码] | [撤销] [关闭]                                    │
 *   └────────────────────────────────────────────────────────────────┘
 *
 *   ┌─ view = 'rotate' ─────────────────────────────────────────────┐
 *   │ ⚠ 提交后旧密码立即失效                                         │
 *   │                                                                │
 *   │ 新密码(可选)                                                  │
 *   │ [                    ]                                         │
 *   │ 留空 → 自动生成强密码                                          │
 *   ├────────────────────────────────────────────────────────────────┤
 *   │                                  [取消] [生成新密码]          │
 *   └────────────────────────────────────────────────────────────────┘
 *
 * # 安全不变量
 *
 * 1. **未改密时不展示 QR**: 已注册设备的明文密码服务端不保存(只剩
 *    Argon2 PHC), connect URI 拼不出来。view='info' 默认显示占位区,
 *    引导用户走改密路径。
 * 2. **改密成功后明文仅在内存里**: `rotateResult` 是 useState 本地状态,
 *    dialog 关闭即清空, 下次打开 dialog 又回到"无明文"。amber 凭据区里
 *    的「备份」按钮一键复制 Server/Username/Password,用户在关闭前自行决定
 *    是否保存 —— 不再强制勾选"我已保存"。
 *
 * # 切换网卡逻辑
 *
 * 与 MobileSyncCredentialModal 一致: 切换不写回 daemon settings, 不重启
 * listener (daemon 永远 bind 0.0.0.0:port)。前端 `buildConnectUri` 用新
 * host 重算 QR 即可。
 */

import {
  AlertTriangle,
  Check,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  Smartphone,
  Trash2,
} from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  listMobileLanInterfaces,
  rotateMobilePassword,
  type LanInterfaceView,
  type MobileDeviceView,
  type MobileSyncError,
  type MobileSyncSettingsView,
  type RotateMobilePasswordResult,
} from '@/api/tauri-command/mobile_sync'
import { BaseUrlChip } from '@/components/device/MobileSyncBaseUrlChip'
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
import { Label } from '@/components/ui/label'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { buildConnectUri } from '@/lib/mobileSyncConnectUri'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-sync-device-dialog')

type View = 'info' | 'rotate'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  device: MobileDeviceView | null
  settings: MobileSyncSettingsView | null
  /** 撤销路径:父组件接住后弹 AlertDialog 二次确认。 */
  onRevoke: (device: MobileDeviceView) => void
  /** rotate 成功后通知父组件刷新设备列表 (lastSeen 等可能未变, 但保持
   *  数据一致性)。 */
  onRotated: () => void
}

const MobileSyncDeviceDialog: React.FC<Props> = ({
  open,
  onOpenChange,
  device,
  settings,
  onRevoke,
  onRotated,
}) => {
  const { t } = useTranslation()

  // ── 视图状态 ────────────────────────────────────────────────────
  const [view, setView] = useState<View>('info')
  const [rotateResult, setRotateResult] = useState<RotateMobilePasswordResult | null>(null)

  // ── 改密表单状态 ────────────────────────────────────────────────
  const [pwdInput, setPwdInput] = useState('')
  const [submitting, setSubmitting] = useState(false)

  // ── 网卡 dropdown ──────────────────────────────────────────────
  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])
  const [selectedHost, setSelectedHost] = useState<string | null>(null)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [backupCopied, setBackupCopied] = useState(false)

  // dialog 打开 / 设备切换 时重置所有 transient 状态。明文密码 (rotateResult)
  // 必须在这一刻清掉 —— 否则用户切换设备后会看到上一台设备的密码。
  useEffect(() => {
    if (open) {
      setView('info')
      setRotateResult(null)
      setPwdInput('')
      setSubmitting(false)
      setSelectedHost(null)
      setPasswordVisible(false)
      setBackupCopied(false)
    }
  }, [open, device?.deviceId])

  // ── 端口 / 首选 host ──────────────────────────────────────────
  const port = useMemo(() => {
    if (settings?.lanPort != null) return String(settings.lanPort)
    return '42720'
  }, [settings?.lanPort])

  const preferredHost = settings?.lanAdvertiseIp ?? null

  // open 时拉一次接口列表
  useEffect(() => {
    if (!open) return
    let cancelled = false
    listMobileLanInterfaces()
      .then(list => {
        if (!cancelled) setLanInterfaces(list)
      })
      .catch(err => log.warn({ err }, 'failed to list LAN interfaces'))
    return () => {
      cancelled = true
    }
  }, [open])

  // dropdown 数据源 = list ∪ preferredHost (去重)。preferredHost 兜底
  // 保证"settings 里偏好的 IP"始终可被点回。
  const dropdownInterfaces = useMemo<LanInterfaceView[]>(() => {
    const seen = new Set<string>()
    const out: LanInterfaceView[] = []
    for (const iface of lanInterfaces) {
      if (!seen.has(iface.ipv4)) {
        seen.add(iface.ipv4)
        out.push(iface)
      }
    }
    if (preferredHost !== null && preferredHost !== '' && !seen.has(preferredHost)) {
      out.push({ name: preferredHost, ipv4: preferredHost })
    }
    return out
  }, [lanInterfaces, preferredHost])

  // selectedHost 初始化
  useEffect(() => {
    if (selectedHost !== null) return
    if (dropdownInterfaces.length === 0) return
    if (preferredHost !== null && dropdownInterfaces.some(i => i.ipv4 === preferredHost)) {
      setSelectedHost(preferredHost)
    } else {
      setSelectedHost(dropdownInterfaces[0].ipv4)
    }
  }, [dropdownInterfaces, preferredHost, selectedHost])

  const effectiveBaseUrl = useMemo(() => {
    const host = selectedHost ?? preferredHost ?? '0.0.0.0'
    return `http://${host}:${port}`
  }, [selectedHost, preferredHost, port])

  // connect URI 仅在 rotateResult 存在时可拼。device.label 保留用作 ConnectUriOther.label。
  // 多候选(docs/planning/mobile-sync-qr-multi-url.md §4): 所选 host 提升为 urls[0],
  // 其后跟公网入口(若配置)与其余网卡候选, 去重保序 —— 改密后的码在其它
  // 网络位置仍然可用。
  const connectUri = useMemo<string | null>(() => {
    if (!rotateResult || !device) return null
    try {
      // Set 保插入序去重: 所选 host → 公网入口 → 其余网卡。
      const candidates = new Set<string>([effectiveBaseUrl])
      if (settings?.lanAdvertiseBaseUrl) {
        candidates.add(settings.lanAdvertiseBaseUrl)
      }
      for (const iface of dropdownInterfaces) {
        candidates.add(`http://${iface.ipv4}:${port}`)
      }
      return buildConnectUri([...candidates], rotateResult.username, rotateResult.password, {
        label: device.label,
        did: rotateResult.deviceId,
        proto: 'syncclipboard',
      })
    } catch (err) {
      log.warn({ err }, 'failed to build connect URI in device dialog')
      return null
    }
  }, [device, effectiveBaseUrl, rotateResult, settings, dropdownInterfaces, port])

  // ── 改密提交 ──────────────────────────────────────────────────
  const handleSubmitRotate = useCallback(async () => {
    if (!device) return
    setSubmitting(true)
    try {
      const result = await rotateMobilePassword({
        deviceId: device.deviceId,
        password: pwdInput.length > 0 ? pwdInput : undefined,
      })
      // 关键: 写入 rotateResult 让 view='info' QR 能拼。pwdInput 清掉以免
      // 用户切回 rotate view 再用旧值二次提交。
      setRotateResult(result)
      setPwdInput('')
      setView('info')
      onRotated()
    } catch (err) {
      log.error({ err, deviceId: device.deviceId }, 'failed to rotate mobile password')
      toast.error(translateRotateError(t, err))
    } finally {
      setSubmitting(false)
    }
  }, [device, onRotated, pwdInput, t])

  // ── 备份(amber 凭据区一键复制) ────────────────────────────────
  const handleBackup = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation()
      if (!rotateResult) return
      const text = `Server: ${effectiveBaseUrl}\nUsername: ${rotateResult.username}\nPassword: ${rotateResult.password}`
      try {
        await navigator.clipboard.writeText(text)
        setBackupCopied(true)
        setTimeout(() => setBackupCopied(false), 1500)
      } catch {
        toast.error('Copy failed')
      }
    },
    [effectiveBaseUrl, rotateResult]
  )

  if (!device) return null

  return (
    <Dialog
      open={open}
      onOpenChange={next => {
        // submitting 时全部锁死避免提交中途关闭。
        if (submitting) return
        onOpenChange(next)
      }}
    >
      <DialogContent
        className="flex max-h-[90vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-md"
        onEscapeKeyDown={e => {
          if (submitting) e.preventDefault()
        }}
        onPointerDownOutside={e => {
          if (submitting) e.preventDefault()
        }}
      >
        <DialogHeader className="px-5 pt-5 pb-3">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-info/10 text-info">
              <Smartphone className="h-5 w-5" />
            </div>
            <div className="min-w-0 flex-1">
              <DialogTitle className="truncate text-left">
                {view === 'rotate'
                  ? t('devices.mobileSync.rotate.dialog.title', { label: device.label })
                  : device.label}
              </DialogTitle>
              <DialogDescription className="truncate font-mono text-xs text-muted-foreground">
                {device.username}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="flex-1 space-y-5 overflow-y-auto px-5 pb-4">
          {view === 'info' ? (
            <InfoView
              device={device}
              effectiveBaseUrl={effectiveBaseUrl}
              dropdownInterfaces={dropdownInterfaces}
              port={port}
              selectedHost={selectedHost}
              onSelectHost={setSelectedHost}
              connectUri={connectUri}
              rotateResult={rotateResult}
              passwordVisible={passwordVisible}
              setPasswordVisible={setPasswordVisible}
              backupCopied={backupCopied}
              onBackup={handleBackup}
            />
          ) : (
            <RotateView
              passwordInput={pwdInput}
              setPasswordInput={setPwdInput}
              submitting={submitting}
            />
          )}
        </div>

        <DialogFooter className="m-0 !flex-row !justify-between gap-2">
          {view === 'info' ? (
            <>
              <Button variant="outline" size="sm" onClick={() => setView('rotate')}>
                <KeyRound className="h-3.5 w-3.5" />
                {t('devices.mobileSync.rotate.button')}
              </Button>
              <div className="flex gap-2">
                <Button variant="destructive" size="sm" onClick={() => onRevoke(device)}>
                  <Trash2 className="h-3.5 w-3.5" />
                  {t('devices.mobileSync.revoke.confirm')}
                </Button>
                <Button size="sm" onClick={() => onOpenChange(false)}>
                  {t('devices.mobileSync.credential.close')}
                </Button>
              </div>
            </>
          ) : (
            <>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setPwdInput('')
                  setView('info')
                }}
                disabled={submitting}
              >
                {t('devices.mobileSync.rotate.dialog.cancel')}
              </Button>
              <Button size="sm" onClick={handleSubmitRotate} disabled={submitting}>
                {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
                {submitting
                  ? t('devices.mobileSync.rotate.dialog.submitting')
                  : t('devices.mobileSync.rotate.dialog.submit')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default MobileSyncDeviceDialog

// ────────────────────────────────────────────────────────────────────
// view = 'info'
// ────────────────────────────────────────────────────────────────────

interface InfoViewProps {
  device: MobileDeviceView
  effectiveBaseUrl: string
  dropdownInterfaces: LanInterfaceView[]
  port: string
  selectedHost: string | null
  onSelectHost: (host: string) => void
  connectUri: string | null
  rotateResult: RotateMobilePasswordResult | null
  passwordVisible: boolean
  setPasswordVisible: (v: boolean) => void
  backupCopied: boolean
  onBackup: (e: React.MouseEvent) => void
}

const InfoView: React.FC<InfoViewProps> = ({
  device,
  effectiveBaseUrl,
  dropdownInterfaces,
  port,
  selectedHost,
  onSelectHost,
  connectUri,
  rotateResult,
  passwordVisible,
  setPasswordVisible,
  backupCopied,
  onBackup,
}) => {
  const { t } = useTranslation()
  const createdAt = formatAbsoluteDateTime(device.createdAtMs)
  const lastSeen =
    device.lastSeenAtMs != null
      ? formatAbsoluteDateTime(device.lastSeenAtMs)
      : t('devices.mobileSync.list.lastSeen.never')

  return (
    <>
      {/* ── 服务地址 + QR ──────────────────────────────────────── */}
      <Section title={t('devices.mobileSync.deviceDialog.sections.serverAddress')}>
        <div className="flex flex-col items-center gap-3 rounded-lg border border-border/60 bg-muted/30 p-4">
          <div className="flex w-full flex-col items-center gap-1.5">
            {dropdownInterfaces.length > 1 && (
              <span className="text-xs text-muted-foreground">
                {t('devices.mobileSync.credential.pair.wifiHint')}
              </span>
            )}
            <BaseUrlChip
              baseUrl={effectiveBaseUrl}
              interfaces={dropdownInterfaces}
              port={port}
              selectedHost={selectedHost}
              onSelect={onSelectHost}
            />
          </div>

          {connectUri !== null ? (
            <>
              <div className="rounded-md bg-white p-3">
                <QRCodeSVG
                  value={connectUri}
                  size={208}
                  aria-label={t('devices.mobileSync.credential.pair.qrAlt')}
                />
              </div>
              <p className="text-center text-xs text-muted-foreground">
                {t('devices.mobileSync.credential.pair.connectHint')}
              </p>
            </>
          ) : (
            <QrPlaceholder />
          )}
        </div>
      </Section>

      {/* ── 凭据(仅 rotateResult 存在时显示) ──────────────────── */}
      {rotateResult && (
        <Section title={t('devices.mobileSync.credential.credentials.title')} accent="amber">
          <div className="space-y-3 rounded-md border border-amber-500/40 bg-amber-500/5 p-3">
            <div className="flex items-start gap-2">
              <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
              <p className="flex-1 text-xs text-amber-700/90 dark:text-amber-400/90">
                {t('devices.mobileSync.rotate.result.warning')}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 shrink-0 border-amber-500/40 text-amber-700 hover:bg-amber-500/10 hover:text-amber-700 dark:text-amber-400 dark:hover:text-amber-400"
                onClick={onBackup}
                aria-label={t('devices.mobileSync.credential.credentials.backup')}
              >
                {backupCopied ? (
                  <>
                    <Check className="h-3.5 w-3.5" />
                    {t('devices.mobileSync.credential.credentials.backupCopied')}
                  </>
                ) : (
                  <>
                    <Copy className="h-3.5 w-3.5" />
                    {t('devices.mobileSync.credential.credentials.backup')}
                  </>
                )}
              </Button>
            </div>

            <CredentialRow
              label={t('devices.mobileSync.credential.username.label')}
              value={rotateResult.username}
            />
            <CredentialRow
              label={t('devices.mobileSync.credential.password.label')}
              value={rotateResult.password}
              secret={!passwordVisible}
              extra={
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label={passwordVisible ? 'hide' : 'show'}
                  title={passwordVisible ? 'hide' : 'show'}
                  onClick={() => setPasswordVisible(!passwordVisible)}
                >
                  {passwordVisible ? (
                    <EyeOff className="h-3.5 w-3.5" />
                  ) : (
                    <Eye className="h-3.5 w-3.5" />
                  )}
                </Button>
              }
            />
          </div>
        </Section>
      )}

      {/* ── 设备信息 ──────────────────────────────────────────── */}
      <Section title={t('devices.mobileSync.deviceDialog.sections.info')}>
        <InfoRow label={t('devices.mobileSync.deviceDialog.fields.createdAt')} value={createdAt} />
        <InfoRow label={t('devices.mobileSync.deviceDialog.fields.lastSeen')} value={lastSeen} />
        {device.lastSeenIp && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.lastSeenIp')}
            value={device.lastSeenIp}
            mono
          />
        )}
        {device.reportedName && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.reportedName')}
            value={device.reportedName}
          />
        )}
        {device.reportedOs && (
          <InfoRow
            label={t('devices.mobileSync.deviceDialog.fields.reportedOs')}
            value={device.reportedOs}
          />
        )}
      </Section>
    </>
  )
}

// ────────────────────────────────────────────────────────────────────
// view = 'rotate'
// ────────────────────────────────────────────────────────────────────

interface RotateViewProps {
  passwordInput: string
  setPasswordInput: (v: string) => void
  submitting: boolean
}

const RotateView: React.FC<RotateViewProps> = ({ passwordInput, setPasswordInput, submitting }) => {
  const { t } = useTranslation()
  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        {t('devices.mobileSync.rotate.dialog.subtitle')}
      </p>

      <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
        <p className="text-xs text-amber-700/90 dark:text-amber-400/90">
          {t('devices.mobileSync.rotate.dialog.warning')}
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="device-dialog-rotate-password">
          {t('devices.mobileSync.rotate.dialog.passwordLabel')}
        </Label>
        <Input
          id="device-dialog-rotate-password"
          type="password"
          autoFocus
          value={passwordInput}
          onChange={e => setPasswordInput(e.target.value)}
          placeholder={t('devices.mobileSync.rotate.dialog.passwordPlaceholder')}
          disabled={submitting}
          autoComplete="new-password"
        />
        <p className="text-xs text-muted-foreground/80">
          {t('devices.mobileSync.rotate.dialog.passwordHelp')}
        </p>
      </div>
    </div>
  )
}

// ────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────

const Section: React.FC<{
  title: string
  accent?: 'default' | 'amber'
  children: React.ReactNode
}> = ({ title, accent = 'default', children }) => (
  <section className="space-y-2">
    <h5
      className={cn(
        'px-1 text-[11px] uppercase tracking-wider',
        accent === 'amber' ? 'text-amber-700/80 dark:text-amber-400/80' : 'text-muted-foreground'
      )}
    >
      {title}
    </h5>
    <div className="space-y-2">{children}</div>
  </section>
)

const InfoRow: React.FC<{ label: string; value: string; mono?: boolean }> = ({
  label,
  value,
  mono,
}) => (
  <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/50 px-3 py-2 text-xs">
    <span className="shrink-0 text-muted-foreground">{label}</span>
    <span className={cn('min-w-0 truncate text-foreground', mono && 'font-mono')} title={value}>
      {value}
    </span>
  </div>
)

const CredentialRow: React.FC<{
  label: string
  value: string
  secret?: boolean
  extra?: React.ReactNode
}> = ({ label, value, secret, extra }) => {
  const display = secret ? value.replace(/./g, '•') : value
  return (
    <div className="flex items-center gap-2">
      <Label className="w-16 shrink-0 text-xs text-muted-foreground">{label}</Label>
      <div className="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-border/60 bg-card px-2 py-1">
        <span
          className={cn('min-w-0 flex-1 truncate font-mono text-sm', secret && 'tracking-widest')}
        >
          {display}
        </span>
        {extra}
        <InlineCopyButton value={value} />
      </div>
    </div>
  )
}

const InlineCopyButton: React.FC<{ value: string }> = ({ value }) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error('Copy failed')
    }
  }, [value])
  const label = copied
    ? t('devices.mobileSync.credential.copied')
    : t('devices.mobileSync.credential.copy')
  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={label}
      title={label}
      onClick={handleCopy}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-emerald-500" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </Button>
  )
}

const QrPlaceholder: React.FC = () => {
  const { t } = useTranslation()
  return (
    <div className="flex h-[208px] w-[208px] flex-col items-center justify-center gap-3 rounded-md border-2 border-dashed border-border/60 bg-card/50 p-4 text-center">
      <KeyRound className="h-8 w-8 text-muted-foreground/50" />
      <p className="text-xs leading-snug text-muted-foreground">
        {t('devices.mobileSync.deviceDialog.qrPlaceholder')}
      </p>
    </div>
  )
}

function formatAbsoluteDateTime(ms: number): string {
  const d = new Date(ms)
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd} ${hh}:${mi}`
}

function translateRotateError(t: ReturnType<typeof useTranslation>['t'], err: unknown): string {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'DEVICE_NOT_FOUND':
        return t('devices.mobileSync.errors.deviceNotFound')
      case 'PASSWORD_TOO_SHORT':
        return t('devices.mobileSync.errors.passwordTooShort', { min: e.min })
      case 'PASSWORD_TOO_LONG':
        return t('devices.mobileSync.errors.passwordTooLong', { max: e.max })
      case 'PASSWORD_HASH_FAILED':
        return t('devices.mobileSync.errors.passwordHashFailed', { message: e.message })
      case 'PERSISTENCE_FAILED':
        return t('devices.mobileSync.errors.persistenceFailed', { message: e.message })
      case 'FACADE_UNAVAILABLE':
        return t('devices.mobileSync.errors.facadeUnavailable')
      default: {
        const message = (e as { message?: string }).message ?? e.code
        return t('devices.mobileSync.errors.unknown', { message })
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('devices.mobileSync.errors.unknown', { message })
}
