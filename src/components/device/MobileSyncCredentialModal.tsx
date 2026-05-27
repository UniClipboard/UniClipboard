/**
 * MobileSyncCredentialModal —— 注册成功后展示一次性凭据 + 引导扫码配对。
 *
 * 设计意图(由 UX 决策树推导, 见 commit message / PR 描述):
 *
 * - **核心使命**: 扫码配对。modal 第一屏 = 大 QR + baseUrl 选择, 其它一切下沉。
 * - **默认起点**: pair (connect URI QR) — 重复添加场景占 99%, 用户大概率已装
 *   客户端。首次用户通过"还没装客户端?"折叠区开门走兜底安装流程。
 * - **撤销下沉**: modal 不再承担"撤销刚注册的设备"职责。X、ESC、点遮罩、
 *   footer"完成"全部走 `onComplete`(保留设备)。用户后悔时去 DevicesPage
 *   的设备卡片上点 revoke — 单一职责, 也避免了 X 按钮"看着像关闭、实际删
 *   设备"的 UX 陷阱。
 * - **多网卡**: 服务地址做成 dropdown 紧贴 QR(语义上 baseUrl 是 QR 的配置
 *   参数, 不是 username/password 那类"凭据"输出)。切换后前端
 *   `buildConnectUri` 重算 QR, 不写回 settings, 不重启 listener。单网卡
 *   机器 dropdown 退化为只读 chip。
 * - **凭据折叠**: username/password 默认折叠在 amber 警告框内, 提供一键
 *   "复制全部备份"。99% 扫码用户看不到凭据噪音, 1% 手动输入用户展开或
 *   一键备份到密码管理器。
 *
 * # 关键不变量(对应 facade 合约 `RegisterMobileShortcutDeviceOutput`):
 * 1. password 是**唯一一次**面向用户的明文回显; 关闭后服务端只剩 PHC。
 * 2. password 永不进 log / 持久化 / analytics(已在 invokeWithTrace
 *    sensitive args redaction 处约束; 本组件再多一层"卸载即丢"的内存
 *    策略, 上层不应把这份对象长期持有)。
 *
 * # connect URI 镜像约束
 * scan QR 的内容由前端 `buildConnectUri()` 实时构造, 与后端
 * `usecases/mobile_sync/connect_uri.rs` 字节级镜像。改任一侧都要同步
 * 另一侧 + golden vector。
 */

import { openUrl } from '@tauri-apps/plugin-opener'
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  Smartphone,
  XIcon,
} from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  listMobileLanInterfaces,
  type LanInterfaceView,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { BaseUrlChip, CopyIconButton } from '@/components/device/MobileSyncBaseUrlChip'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { buildConnectUri } from '@/lib/mobileSyncConnectUri'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-sync-credential')

// 产品级常量 — 不本地化, 直接面向用户。
// iOS App 当前在 TestFlight public beta, 用户必须先装 TestFlight 才能装本
// App。短期内是 iOS 推荐路径。
const TESTFLIGHT_URL = 'https://testflight.apple.com/join/nyNQ8dQe'
// Android 客户端是 SyncClipboard 协议兼容的 fork, APK 走 GitHub releases。
const ANDROID_RELEASES_URL = 'https://github.com/UniClipboard/uc-android/releases/latest'

interface Props {
  /** 凭据 payload。`null` 表示 modal 关闭。 */
  payload: RegisterMobileDeviceResult | null
  /** 关闭 modal(保留设备)。X / ESC / 点遮罩 / footer 主按钮共用此回调。
   *  "撤销刚注册的设备"已下沉到 DevicesPage 设备卡片上的 revoke 按钮,
   *  本组件不再承担该职责。 */
  onComplete: () => void
}

const MobileSyncCredentialModal: React.FC<Props> = ({ payload, onComplete }) => {
  const { t } = useTranslation()

  // ── 多网卡服务地址 ──────────────────────────────────────────────────
  const [lanInterfaces, setLanInterfaces] = useState<LanInterfaceView[]>([])
  const [selectedHost, setSelectedHost] = useState<string | null>(null)

  // 拆 payload.baseUrl: host 是 dropdown 切换的对象, port 跟随 daemon 不变。
  const { payloadHost, payloadPort } = useMemo(() => {
    if (!payload) return { payloadHost: '', payloadPort: '' }
    try {
      const u = new URL(payload.baseUrl)
      return { payloadHost: u.hostname, payloadPort: u.port }
    } catch {
      return { payloadHost: '', payloadPort: '' }
    }
  }, [payload])

  // payload 切换(新设备)时重置选择, 让初始化 effect 再跑一次。
  useEffect(() => {
    setSelectedHost(null)
  }, [payload?.deviceId])

  // modal 打开时拉一次 LAN 接口列表。失败仅日志, 回退到只读 chip 不影响
  // "完成保存"流程(凭据本身是 baseUrl 无关的)。
  useEffect(() => {
    if (!payload) return
    let cancelled = false
    listMobileLanInterfaces()
      .then(list => {
        if (!cancelled) setLanInterfaces(list)
      })
      .catch(err => log.warn({ err }, 'failed to list LAN interfaces'))
    return () => {
      cancelled = true
    }
  }, [payload?.deviceId])

  // dropdown 数据源 = 后端 list 接口 ∪ payload.baseUrl 的 host(去重)。
  // 双保险:
  // 1. 后端 list 拉空(权限/timing/接口故障)时, payloadHost 兜底成单元素
  //    候选 — 用户至少能在 dropdown 里看到当前正用的 IP, 不会出现
  //    "看着像 chip 但点不开"的体验断裂。
  // 2. payloadHost 不在 list 里时(比如机器多网卡, daemon 注册时挑了
  //    某个但用户在前端不慎切到另一个), 也并入候选, 让"当前在用的"始终
  //    可被点回。
  // 即使只剩 1 个候选, 也走 dropdown(UI 一致性 > 严格无意义控件), 用户
  // 点开能看清"这是这台机器仅有的 LAN 候选"。
  const dropdownInterfaces = useMemo<LanInterfaceView[]>(() => {
    const seen = new Set<string>()
    const out: LanInterfaceView[] = []
    for (const iface of lanInterfaces) {
      if (!seen.has(iface.ipv4)) {
        seen.add(iface.ipv4)
        out.push(iface)
      }
    }
    if (payloadHost !== '' && !seen.has(payloadHost)) {
      // 后端没列出当前 host:用 host 自身作 name(没别的可显示)。
      out.push({ name: payloadHost, ipv4: payloadHost })
    }
    return out
  }, [lanInterfaces, payloadHost])

  // selectedHost 初始化(基于派生后的 dropdownInterfaces, 包含 payloadHost
  // 兜底):
  // 1) payloadHost 在候选里 → 保持(用户首次注册时挑过的不要被覆盖)
  // 2) 未命中且列表非空 → 选第一个
  // 3) 列表空 → 保持 null, BaseUrlChip 走只读 fallback
  useEffect(() => {
    if (selectedHost !== null) return
    if (dropdownInterfaces.length === 0) return
    if (dropdownInterfaces.some(i => i.ipv4 === payloadHost)) {
      setSelectedHost(payloadHost)
    } else {
      setSelectedHost(dropdownInterfaces[0].ipv4)
    }
  }, [dropdownInterfaces, payloadHost, selectedHost])

  // 派生当前生效的 baseUrl + connect URI。
  // 有 selectedHost 且 port 已知 → 前端 buildConnectUri 重算(跟 Rust 端
  // 字节级镜像)。否则回退用 payload 自带的字段。
  const effectiveBaseUrl = useMemo(() => {
    if (!payload) return ''
    if (selectedHost === null || payloadPort === '') return payload.baseUrl
    return `http://${selectedHost}:${payloadPort}`
  }, [payload, selectedHost, payloadPort])

  const effectiveConnectUri = useMemo(() => {
    if (!payload) return ''
    // 与后端 register_device.rs 的 ConnectUriOther{label, did} 一致。
    try {
      return buildConnectUri(effectiveBaseUrl, payload.username, payload.password, {
        label: payload.label,
        did: payload.deviceId,
      })
    } catch (err) {
      log.warn({ err }, 'failed to rebuild connect URI, falling back to payload')
      return payload.connectUri
    }
  }, [payload, effectiveBaseUrl])

  const handleComplete = useCallback(() => {
    setSelectedHost(null)
    onComplete()
  }, [onComplete])

  if (!payload) return null

  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        // sm:max-w-lg 必须显式覆盖默认 sm:max-w-sm —— 长 URL/QR/折叠区都
        // 需要 32rem 才不挤; max-h + 内层滚动是兜底, 但默认状态(两个折叠
        // 区都收着)应当在 90vh 内, 无滚动条。
        className="flex max-h-[90vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-lg"
        // X / ESC / 点遮罩 全部走"完成"(保留设备)。撤销操作下沉到
        // DevicesPage, modal 不再承担。
        onEscapeKeyDown={() => handleComplete()}
        onPointerDownOutside={() => handleComplete()}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="absolute top-2 right-2"
          aria-label={t('devices.mobileSync.credential.closeAria')}
          onClick={handleComplete}
        >
          <XIcon />
        </Button>

        <DialogHeader className="px-5 pt-5 pb-3">
          <DialogTitle className="pr-8 text-base">
            {t('devices.mobileSync.credential.title', { label: payload.label })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t('devices.mobileSync.credential.subtitle')}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 space-y-4 overflow-y-auto px-5 pb-4">
          {/* 主区域:大 QR + baseUrl 配置 + 提示文案 */}
          <ScanArea
            connectUri={effectiveConnectUri}
            qrAlt={t('devices.mobileSync.credential.pair.qrAlt')}
            connectHint={t('devices.mobileSync.credential.pair.connectHint')}
            baseUrl={effectiveBaseUrl}
            interfaces={dropdownInterfaces}
            port={payloadPort}
            selectedHost={selectedHost}
            onSelect={setSelectedHost}
          />

          {/* 折叠 1: 还没装客户端?(中性色) */}
          <NoClientCollapsible installQrCodePngBase64={payload.installQrCodePngBase64} />

          {/* 折叠 2: 凭据(amber 警告色, 含 [备份] 按钮 + 折叠的 user/pwd) */}
          <CredentialsCollapsible
            baseUrl={effectiveBaseUrl}
            username={payload.username}
            password={payload.password}
          />
        </div>

        <DialogFooter className="m-0">
          <Button onClick={handleComplete}>{t('devices.mobileSync.credential.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ────────────────────────────────────────────────────────────────────────
// Scan 主区域 —— 大 QR + baseUrl chip dropdown + hint
// ────────────────────────────────────────────────────────────────────────

interface ScanAreaProps {
  connectUri: string
  qrAlt: string
  connectHint: string
  baseUrl: string
  interfaces: LanInterfaceView[]
  port: string
  selectedHost: string | null
  onSelect: (host: string) => void
}

const ScanArea: React.FC<ScanAreaProps> = ({
  connectUri,
  qrAlt,
  connectHint,
  baseUrl,
  interfaces,
  port,
  selectedHost,
  onSelect,
}) => {
  const { t } = useTranslation()
  return (
    <div className="flex flex-col items-center gap-3 rounded-lg border border-border/60 bg-muted/30 p-5">
      {/* 主 QR — 224px, scanability 比原 176px 提升一截; bg-white p-3
          是 QR scanner 必备的"白底 + quiet zone" */}
      <div className="rounded-md bg-white p-3">
        <QRCodeSVG value={connectUri} size={224} aria-label={qrAlt} />
      </div>

      {/* baseUrl chip dropdown — 多 IP 可切, 单 IP 退化为只读 */}
      <div className="flex w-full flex-col items-center gap-1.5">
        {interfaces.length > 1 && (
          <span className="text-xs text-muted-foreground">
            {t('devices.mobileSync.credential.pair.wifiHint')}
          </span>
        )}
        <BaseUrlChip
          baseUrl={baseUrl}
          interfaces={interfaces}
          port={port}
          selectedHost={selectedHost}
          onSelect={onSelect}
        />
      </div>

      <p className="text-center text-xs text-muted-foreground">{connectHint}</p>
    </div>
  )
}

// ────────────────────────────────────────────────────────────────────────
// 折叠 1: 还没装客户端?
// ────────────────────────────────────────────────────────────────────────

interface NoClientCollapsibleProps {
  /** SyncClipboard 快捷指令的安装 QR (后端渲染 base64 PNG)。 */
  installQrCodePngBase64: string
}

type NoClientTab = 'ios' | 'android'

/**
 * "还没装客户端?" 折叠区 —— 展开后是 iOS / Android 二选一 tab。
 *
 * 设计意图: 平台分流明确, 每个 tab 主操作 = 大 QR 扫码下载对应 App
 * (iOS → TestFlight 邀请链接 QR; Android → GitHub Releases APK 页 QR)。
 * 用户在桌面上不需要手动复制 URL, 拿手机对屏一扫即可在浏览器打开下载入口。
 *
 * iOS tab 多一个二级"或安装快捷指令"link, 作为对不愿/不能装 App 的兜底
 * (装一次后任何"扫码接入" QR 都能用)。Android 没有这条兜底 — uc-android
 * 是 SyncClipboard 协议兼容的 fork, 不需要 shortcut。
 */
const NoClientCollapsible: React.FC<NoClientCollapsibleProps> = ({ installQrCodePngBase64 }) => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [tab, setTab] = useState<NoClientTab>('ios')

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex w-full items-center justify-between rounded-md border border-border/60 bg-card px-3 py-2 text-sm hover:bg-accent/50"
        >
          <span className="flex items-center gap-2">
            <Smartphone className="h-4 w-4 text-muted-foreground" />
            {t('devices.mobileSync.credential.noClient.title')}
          </span>
          {open ? (
            <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 text-muted-foreground" />
          )}
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-2 rounded-md border border-border/40 bg-muted/20 p-3">
        <Tabs value={tab} onValueChange={v => setTab(v as NoClientTab)}>
          <TabsList className="w-full">
            <TabsTrigger value="ios">
              {t('devices.mobileSync.credential.noClient.tabs.ios')}
            </TabsTrigger>
            <TabsTrigger value="android">
              {t('devices.mobileSync.credential.noClient.tabs.android')}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="ios" className="mt-3 space-y-3">
            <ScanToDownloadPanel
              qrValue={TESTFLIGHT_URL}
              qrAlt={t('devices.mobileSync.credential.noClient.ios.scanQrAlt')}
              caption={t('devices.mobileSync.credential.noClient.ios.scanLabel')}
              browserLink={t('devices.mobileSync.credential.noClient.ios.openInBrowser')}
              browserHref={TESTFLIGHT_URL}
            />
            {/* 兜底:不想装 App 的用户走快捷指令路径(只装一次后续都通用)。
                视觉上是次要 link + 小 QR icon 弹 popover, 不抢 App QR 主体。 */}
            <div className="flex items-center justify-between gap-2 border-t border-border/40 pt-2 text-xs">
              <span className="text-muted-foreground">
                {t('devices.mobileSync.credential.noClient.ios.shortcutFallback')}
              </span>
              <QrPopoverButton
                ariaLabel={t('devices.mobileSync.credential.noClient.ios.shortcutQrAria')}
                imageSrc={`data:image/png;base64,${installQrCodePngBase64}`}
                imageAlt={t('devices.mobileSync.credential.noClient.ios.shortcutQrAlt')}
              />
            </div>
          </TabsContent>

          <TabsContent value="android" className="mt-3">
            <ScanToDownloadPanel
              qrValue={ANDROID_RELEASES_URL}
              qrAlt={t('devices.mobileSync.credential.noClient.android.scanQrAlt')}
              caption={t('devices.mobileSync.credential.noClient.android.scanLabel')}
              browserLink={t('devices.mobileSync.credential.noClient.android.openInBrowser')}
              browserHref={ANDROID_RELEASES_URL}
            />
          </TabsContent>
        </Tabs>
      </CollapsibleContent>
    </Collapsible>
  )
}

interface ScanToDownloadPanelProps {
  qrValue: string
  qrAlt: string
  caption: string
  browserLink: string
  browserHref: string
}

/**
 * 通用"扫码下载 App"面板 —— iOS / Android tab 共用:
 * - 大 QR (160px) 居中, 桌面屏对手机摄像头扫码可达
 * - 下面一行 caption 说明"扫码安装什么"
 * - 一行 outline 的"在浏览器打开"次要按钮, 给鼠标用户兜底(他们也能直接在
 *   桌面浏览器登录 GitHub / Apple ID 完成下载流程)
 */
const ScanToDownloadPanel: React.FC<ScanToDownloadPanelProps> = ({
  qrValue,
  qrAlt,
  caption,
  browserLink,
  browserHref,
}) => (
  <div className="flex flex-col items-center gap-3">
    <div className="rounded-md bg-white p-2">
      <QRCodeSVG value={qrValue} size={160} aria-label={qrAlt} />
    </div>
    <p className="text-center text-xs text-foreground">{caption}</p>
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="h-7 text-xs"
      onClick={() =>
        openUrl(browserHref).catch(err =>
          log.warn({ err, href: browserHref }, 'failed to open URL')
        )
      }
    >
      <ExternalLink className="h-3 w-3" />
      {browserLink}
    </Button>
  </div>
)

interface QrPopoverButtonProps {
  ariaLabel: string
  /** 优先级 1: 直接给 PNG base64 (后端预渲) */
  imageSrc?: string
  /** 优先级 2: 给 SVG value, 前端 qrcode.react 现渲 */
  svgValue?: string
  imageAlt: string
}

/**
 * 一个 📷 icon 按钮, 点击弹 popover 显示 QR。popover 内 QR 用 200px,
 * 桌面屏对着扫足够; 不需要再大 — 一旦超过 ~240px, popover 自身高度会
 * 顶到 modal 边界, 看着拥挤。
 */
const QrPopoverButton: React.FC<QrPopoverButtonProps> = ({
  ariaLabel,
  imageSrc,
  svgValue,
  imageAlt,
}) => (
  <Popover>
    <PopoverTrigger asChild>
      <Button type="button" size="icon-sm" variant="ghost" aria-label={ariaLabel} title={ariaLabel}>
        <Smartphone className="h-3.5 w-3.5" />
      </Button>
    </PopoverTrigger>
    <PopoverContent className="w-auto p-3" align="end">
      <div className="rounded bg-white p-2">
        {imageSrc !== undefined ? (
          <img src={imageSrc} alt={imageAlt} className="h-48 w-48" />
        ) : (
          <QRCodeSVG value={svgValue ?? ''} size={192} aria-label={imageAlt} />
        )}
      </div>
    </PopoverContent>
  </Popover>
)

// ────────────────────────────────────────────────────────────────────────
// 折叠 2: 凭据(amber 警告色, 一键备份 + 折叠 user/pwd)
// ────────────────────────────────────────────────────────────────────────

interface CredentialsCollapsibleProps {
  baseUrl: string
  username: string
  password: string
}

const CredentialsCollapsible: React.FC<CredentialsCollapsibleProps> = ({
  baseUrl,
  username,
  password,
}) => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [backupCopied, setBackupCopied] = useState(false)

  const handleBackup = useCallback(
    async (e: React.MouseEvent) => {
      // 点 [备份] 不应该展开/折叠 — 它是折叠 header 上的旁置 action。
      e.stopPropagation()
      const text = `Server: ${baseUrl}\nUsername: ${username}\nPassword: ${password}`
      try {
        await navigator.clipboard.writeText(text)
        setBackupCopied(true)
        setTimeout(() => setBackupCopied(false), 1500)
      } catch {
        toast.error('Copy failed')
      }
    },
    [baseUrl, username, password]
  )

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      {/* amber 边框 + amber/5 背景 = "这里有要紧的东西"。即使不展开,
          色块也会被动入眼, 比单纯灰色折叠更能传达"密码即将丢失"。 */}
      <div className="rounded-md border border-amber-500/40 bg-amber-500/5">
        <div className="flex items-center gap-2 px-3 py-2">
          <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
          <CollapsibleTrigger asChild>
            <button
              type="button"
              className="flex flex-1 items-center justify-between gap-2 text-left text-sm"
            >
              <span className="font-medium text-amber-700 dark:text-amber-400">
                {t('devices.mobileSync.credential.credentials.title')}
                <span className="ml-1.5 text-xs font-normal text-amber-700/80 dark:text-amber-400/80">
                  {t('devices.mobileSync.credential.credentials.warning')}
                </span>
              </span>
              {open ? (
                <ChevronDown className="h-3.5 w-3.5 text-amber-700/70 dark:text-amber-400/70" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 text-amber-700/70 dark:text-amber-400/70" />
              )}
            </button>
          </CollapsibleTrigger>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 shrink-0 border-amber-500/40 text-amber-700 hover:bg-amber-500/10 hover:text-amber-700 dark:text-amber-400 dark:hover:text-amber-400"
            onClick={handleBackup}
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

        <CollapsibleContent className="space-y-2 border-t border-amber-500/30 px-3 py-3">
          <CredentialField
            label={t('devices.mobileSync.credential.username.label')}
            value={username}
            mono
          />
          <CredentialField
            label={t('devices.mobileSync.credential.password.label')}
            value={password}
            mono
            secret={!passwordVisible}
            extraActions={
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label={passwordVisible ? 'hide' : 'show'}
                title={passwordVisible ? 'hide' : 'show'}
                onClick={() => setPasswordVisible(v => !v)}
              >
                {passwordVisible ? (
                  <EyeOff className="h-3.5 w-3.5" />
                ) : (
                  <Eye className="h-3.5 w-3.5" />
                )}
              </Button>
            }
          />
        </CollapsibleContent>
      </div>
    </Collapsible>
  )
}

// ────────────────────────────────────────────────────────────────────────
// CredentialField —— amber 凭据区里的"键 + 值 + 复制"行
// ────────────────────────────────────────────────────────────────────────

interface CredentialFieldProps {
  label: string
  value: string
  mono?: boolean
  secret?: boolean
  extraActions?: React.ReactNode
}

const CredentialField: React.FC<CredentialFieldProps> = ({
  label,
  value,
  mono,
  secret,
  extraActions,
}) => {
  const display = secret ? value.replace(/./g, '•') : value

  return (
    <div className="flex items-center gap-2">
      <Label className="w-16 shrink-0 text-xs text-muted-foreground">{label}</Label>
      <div className="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-border/60 bg-card px-2 py-1">
        <span
          className={cn(
            'min-w-0 flex-1 truncate text-sm',
            mono && 'font-mono',
            secret && 'tracking-widest'
          )}
        >
          {display}
        </span>
        {extraActions}
        <CopyIconButton value={value} />
      </div>
    </div>
  )
}

export default MobileSyncCredentialModal
