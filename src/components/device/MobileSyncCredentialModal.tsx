/**
 * MobileSyncCredentialModal —— 注册成功后展示一次性凭据。
 *
 * 关键不变量(对应 facade 合约,见 RegisterMobileShortcutDeviceOutput 注释):
 * 1. password 字段是**唯一一次**面向用户的明文回显;关闭后服务端只剩 PHC,
 *    无法再取回。
 * 2. 右上角 ✕ 丢弃本次注册(撤销设备);右下角「完成」须勾选「我已保存」
 *    才确认保留设备并关闭;ESC / 点遮罩仍拦截并提示勾选。
 * 3. password 永远不进 log / 持久化 / analytics(已在 invokeWithTrace 的
 *    sensitive args redaction 处约束;本组件再多一层"卸载即丢"的内存策略,
 *    上层不应把这份对象长期持有)。
 *
 * tab 不按"平台"分,按"接入方式"分 —— connect URI QR 平台无关 (iOS App 与
 * 任何 SyncClipboard 协议客户端都能扫,Android 第三方应用同理),所以早期
 * "iOS / Android" 的分法没意义且会让 Android 用户看到一个空 tab。新分法:
 * - 「扫码接入」 (默认):
 *   - 主操作: connect URI 二维码 (uniclipboard://connect?v=1&svc=mobile-sync&p=...)
 *   - iPhone 上的 UniClipboard 原生 App 或 SyncClipboard 快捷指令扫到后一次性
 *     解出 url/user/pwd 直接填三栏 —— 替代旧版"用户肉眼抄写"。后端 DTO
 *     `qrCodePngBase64` 自阶段 2 起编码的就是 connect URI。
 * - 「安装快捷指令」 (兜底, 一次性):
 *   - 没装 iOS App 的用户兜底走快捷指令路径,需先把模板装到 iPhone 上 —— 装一次
 *     之后任何"扫码接入" QR 都能用。
 *   - 主 QR 是 install URL 的二维码 (后端 DTO `installQrCodePngBase64` 阶段 5 引入),
 *     iPhone 相机直扫即可安装;桌面端打开 iCloud 共享链接无意义,所以也保留
 *     install URL 的文字 + 复制按钮 (CredentialField), 让用户能复制到别处。
 *
 * 关键不变量(对应 facade 合约,见 RegisterMobileShortcutDeviceOutput 注释):
 * 1. password 字段是**唯一一次**面向用户的明文回显;关闭后服务端只剩 PHC,
 *    无法再取回。
 * 2. 右上角 ✕ 丢弃本次注册(撤销设备);右下角「完成」须勾选「我已保存」
 *    才确认保留设备并关闭;ESC / 点遮罩仍拦截并提示勾选。
 * 3. password 永远不进 log / 持久化 / analytics(已在 invokeWithTrace 的
 *    sensitive args redaction 处约束;本组件再多一层"卸载即丢"的内存策略,
 *    上层不应把这份对象长期持有)。
 */

import { openUrl } from '@tauri-apps/plugin-opener'
import { AlertTriangle, Check, Copy, ExternalLink, Eye, EyeOff, XIcon } from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useCallback, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { type RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('mobile-sync-credential')

// 官网下载页 — 该页根据 UA 提供 iOS / Android 两个平台的安装入口,
// 桌面端用户直接扫即可在手机上打开。URL 是产品级常量,不本地化。
const DOWNLOAD_PAGE_URL = 'https://www.uniclipboard.app/download'

interface Props {
  /**
   * 凭据 payload。`null` 表示 modal 关闭(等价 open=false)。父组件清空
   * payload 即关闭 modal,本组件不持有任何引用。
   */
  payload: RegisterMobileDeviceResult | null
  /** 用户点 ✕ 放弃:上层应撤销刚注册的设备。 */
  onDiscard: (deviceId: string) => void | Promise<void>
  /** 用户勾选已保存并点「完成」:保留设备,仅关闭凭据展示。 */
  onComplete: () => void
}

type OnboardingTab = 'scan' | 'shortcut'

// 扫码接入是两步流程: step 1 引导用户在手机上扫"下载页 QR"装 App,
// step 2 才扫 connect URI 配对。第一次接入的用户根本没装 App,直接给
// connect QR 是哑的 —— 必须先把"去哪下载"这件事讲清楚。
type ScanStep = 'download' | 'pair'

const MobileSyncCredentialModal: React.FC<Props> = ({ payload, onDiscard, onComplete }) => {
  const { t } = useTranslation()
  const [acknowledged, setAcknowledged] = useState(false)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [activeTab, setActiveTab] = useState<OnboardingTab>('scan')
  const [scanStep, setScanStep] = useState<ScanStep>('download')
  // 用户尝试关闭但未勾选时的 inline 提示。toast 在 modal 遮罩下很容易被忽视,
  // 改成把红色高亮 + 错误文本直接挂在勾选框上,视线一定会被引到下一步操作。
  const [hintActive, setHintActive] = useState(false)
  const acknowledgeRef = useRef<HTMLLabelElement>(null)

  const resetLocalState = useCallback(() => {
    setAcknowledged(false)
    setPasswordVisible(false)
    setActiveTab('scan')
    setScanStep('download')
    setHintActive(false)
  }, [])

  const handleDiscard = useCallback(() => {
    if (!payload) return
    const { deviceId } = payload
    resetLocalState()
    void onDiscard(deviceId)
  }, [onDiscard, payload, resetLocalState])

  const handleComplete = useCallback(() => {
    resetLocalState()
    onComplete()
  }, [onComplete, resetLocalState])

  const handleAcknowledge = useCallback((v: boolean) => {
    setAcknowledged(v)
    if (v) setHintActive(false)
  }, [])

  const flagUnacknowledged = useCallback(() => {
    setHintActive(true)
    acknowledgeRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }, [])

  const tryClose = useCallback(() => {
    if (!acknowledged) {
      flagUnacknowledged()
      return
    }
    handleComplete()
  }, [acknowledged, flagUnacknowledged, handleComplete])

  if (!payload) return null

  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        // sm:max-w-lg 必须显式覆盖 DialogContent 默认的 sm:max-w-sm,
        // 否则 64rem 长 install URL 在 24rem 容器里会撑爆。max-h + 内层滚动
        // 防止列表 + QR + 4 行凭据在小窗口下溢出底部。
        className="flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-lg"
        // 拦截 ESC / 点击遮罩关闭 —— 必须走勾选门;同时点亮 inline 提示,
        // 让用户立刻看到为什么被挡住
        onEscapeKeyDown={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
        onPointerDownOutside={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
        onInteractOutside={e => {
          if (!acknowledged) {
            e.preventDefault()
            flagUnacknowledged()
          }
        }}
      >
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="absolute top-2 right-2"
          aria-label={t('devices.mobileSync.credential.dismiss')}
          onClick={handleDiscard}
        >
          <XIcon />
        </Button>
        <DialogHeader className="px-4 pt-4 pb-2">
          <DialogTitle>{t('devices.mobileSync.credential.title')}</DialogTitle>
          {/* 视觉上不需要副标题 — 警告横幅 + tab + stepper 已经讲明白了。
              但 Radix DialogContent 要求至少有 description (否则 a11y warn),
              这里用 sr-only 给屏幕阅读器。 */}
          <DialogDescription className="sr-only">
            {t('devices.mobileSync.credential.subtitle')}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 space-y-3 overflow-y-auto px-4 py-2">
          {/* 警告横幅 — 只留 title 一句。body ("关闭后无法再查看,如不慎遗失需
              撤销并重新添加") 删了: title 已经传达核心,recovery 路径在 device
              列表的撤销按钮上,信息不会失传。 */}
          <div className="flex items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm font-medium text-amber-700 dark:text-amber-400">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            <span>{t('devices.mobileSync.credential.warning.title')}</span>
          </div>

          {/* 接入方式 tab —— 凭据 (URL/user/pwd) 共用,只切换"扫什么 QR" */}
          <Tabs value={activeTab} onValueChange={v => setActiveTab(v as OnboardingTab)}>
            <TabsList className="w-full">
              <TabsTrigger value="scan">
                {t('devices.mobileSync.credential.platforms.scan')}
              </TabsTrigger>
              <TabsTrigger value="shortcut">
                {t('devices.mobileSync.credential.platforms.shortcut')}
              </TabsTrigger>
            </TabsList>

            {/* Tab A: 扫码接入 (默认主路径) — 两步 stepper
                  step 1 = 下载 App (QR 指向官网下载页, 页面根据 UA 区分 iOS/Android)
                  step 2 = 扫码配对 (QR = connect URI, qrCodePngBase64 由后端渲染)
                connect URI 平台无关 —— iOS App、SyncClipboard 快捷指令、Android
                客户端均可解; 但前置必须有 App, 所以 step 1 把"去哪下载"先讲清楚。
                布局精简: 取消独立的提示卡, hint 内嵌到 QR 框顶端, "浏览器打开"
                变成右上角小图标; pair step 不放"返回"按钮 — stepper 第 1 步可
                点回退, 不需要重复入口。 */}
            <TabsContent value="scan" className="mt-3 space-y-3">
              <ScanStepper currentStep={scanStep} onSelect={setScanStep} />

              {scanStep === 'download' ? (
                <>
                  <QrPanel
                    hint={t('devices.mobileSync.credential.scan.download.hint')}
                    topRight={
                      <Button
                        type="button"
                        size="icon-sm"
                        variant="ghost"
                        aria-label={t(
                          'devices.mobileSync.credential.scan.download.openInBrowserAria'
                        )}
                        title={t('devices.mobileSync.credential.scan.download.openInBrowserAria')}
                        onClick={() =>
                          openUrl(DOWNLOAD_PAGE_URL).catch(err =>
                            log.error({ err }, 'Failed to open download page URL')
                          )
                        }
                      >
                        <ExternalLink className="h-3.5 w-3.5" />
                      </Button>
                    }
                  >
                    {/* 前端用 qrcode.react 现渲: download URL 是静态产品常量,
                        每次都让后端编码一份 base64 PNG 是浪费。size 176 对应
                        connect QR 显示 (h-48 w-48 减去 p-2)。 */}
                    <QRCodeSVG
                      value={DOWNLOAD_PAGE_URL}
                      size={176}
                      aria-label={t('devices.mobileSync.credential.scan.download.qrAlt')}
                    />
                  </QrPanel>
                  <Button type="button" className="w-full" onClick={() => setScanStep('pair')}>
                    {t('devices.mobileSync.credential.scan.download.next')}
                  </Button>
                </>
              ) : (
                <QrPanel hint={t('devices.mobileSync.credential.scan.pair.hint')}>
                  <img
                    src={`data:image/png;base64,${payload.qrCodePngBase64}`}
                    alt={t('devices.mobileSync.credential.scan.pair.qrAlt')}
                    className="h-44 w-44 rounded bg-white p-2"
                  />
                </QrPanel>
              )}
            </TabsContent>

            {/* Tab B: 安装快捷指令 (一次性兜底)
                installQrCodePngBase64 自后端阶段 5 起单独输出, 让 iPhone 相机
                直接扫码安装 —— 避免用户在桌面上肉眼抄长 iCloud 链接到 Safari。
                同时保留 install URL 文本 + 复制按钮 (CredentialField), 让用户
                能复制链接到 IM / 笔记里日后再装。 */}
            <TabsContent value="shortcut" className="mt-3 space-y-4">
              <div className="space-y-2 rounded-md border border-border/60 bg-card/50 p-3">
                <p className="text-sm font-medium">
                  {t('devices.mobileSync.credential.shortcut.title')}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t('devices.mobileSync.credential.shortcut.body')}
                </p>
              </div>
              <div className="flex flex-col items-center gap-2 rounded-md border border-border/60 bg-muted/30 p-4">
                <Label className="text-xs uppercase tracking-wider text-muted-foreground">
                  {t('devices.mobileSync.credential.shortcut.qr.label')}
                </Label>
                <img
                  src={`data:image/png;base64,${payload.installQrCodePngBase64}`}
                  alt={t('devices.mobileSync.credential.shortcut.qr.alt')}
                  className="h-48 w-48 rounded bg-white p-2"
                />
              </div>
              <CredentialField
                label={t('devices.mobileSync.credential.shortcut.linkLabel')}
                value={payload.installUrl}
                mono
              />
            </TabsContent>
          </Tabs>

          {/* 共用凭据(放 Tabs 外,两个 tab 都能看到) */}
          <div className="space-y-3">
            {/* Server URL */}
            <CredentialField
              label={t('devices.mobileSync.credential.baseUrl.label')}
              value={payload.baseUrl}
              mono
            />

            {/* Username */}
            <CredentialField
              label={t('devices.mobileSync.credential.username.label')}
              value={payload.username}
              mono
            />

            {/* Password — 默认隐藏,点眼睛切换显示;无论显示与否都可以复制 */}
            <CredentialField
              label={t('devices.mobileSync.credential.password.label')}
              value={payload.password}
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
          </div>

          {/* 强制勾选 —— hintActive 时用 primary(蓝色)而非 destructive(红色)
              高亮:红色容易被误读为"错误/disabled",蓝色 + ring + 阴影才是
              "请点这里"的可操作信号。错误文本不放这里,挪到 footer 上方,
              避免视觉重心从 checkbox 上移走。 */}
          <label
            ref={acknowledgeRef}
            className={cn(
              'flex items-start gap-2 rounded-md border bg-card p-3 transition-all',
              hintActive
                ? 'border-primary bg-primary/10 ring-4 ring-primary/30 shadow-md shadow-primary/20'
                : 'border-border/60'
            )}
          >
            <Checkbox
              checked={acknowledged}
              onCheckedChange={v => handleAcknowledge(v === true)}
              className={cn('mt-0.5', hintActive && 'border-primary ring-3 ring-primary/40')}
            />
            <span className={cn('text-sm', hintActive && 'font-medium text-primary')}>
              {t('devices.mobileSync.credential.confirmSaved')}
            </span>
          </label>
        </div>

        {/* 未勾选时关闭被挡 —— 在 footer 上方挂一条横幅,紧贴关闭按钮,
            让用户立刻知道"按了没反应"的原因 */}
        {hintActive && (
          <div
            className="border-t bg-destructive/5 px-4 py-2 text-xs text-destructive"
            role="alert"
          >
            {t('devices.mobileSync.credential.closeBlocked')}
          </div>
        )}

        <DialogFooter className="m-0">
          {/* 按钮始终启用:点击未勾选时触发 inline 提示,而不是冷冰冰的 disabled */}
          <Button onClick={tryClose}>{t('devices.mobileSync.credential.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

interface CredentialFieldProps {
  label: string
  value: string
  /** 用 monospace 字体显示 —— 适合 URL / username / password 等不可读错的字符串。 */
  mono?: boolean
  /** 当前是否要遮罩(只对 password 字段有意义)。 */
  secret?: boolean
  /** 复制按钮左侧的额外动作(例如显示/隐藏密码切换)。 */
  extraActions?: React.ReactNode
}

const CredentialField: React.FC<CredentialFieldProps> = ({
  label,
  value,
  mono,
  secret,
  extraActions,
}) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      // 1.5s 后还原 —— 让用户能再次复制
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error('Copy failed')
    }
  }, [value])

  const display = secret ? value.replace(/./g, '•') : value

  return (
    // Inline layout: label 在左, value 框在右, 一行一个字段。比 stacked
    // (label 在上, 框在下) 节省一半垂直空间, 跟整体精简方向一致。
    // Label 用固定宽度 w-16 让三个字段对齐, value 框 min-w-0 + truncate 处理
    // 长 URL 不溢出。
    <div className="flex items-center gap-2">
      <Label className="w-16 shrink-0 text-xs text-muted-foreground">{label}</Label>
      <div className="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-border/60 bg-card px-2 py-1">
        <span
          className={`min-w-0 flex-1 truncate text-sm ${mono ? 'font-mono' : ''} ${
            secret ? 'tracking-widest' : ''
          }`}
        >
          {display}
        </span>
        {extraActions}
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={
            copied
              ? t('devices.mobileSync.credential.copied')
              : t('devices.mobileSync.credential.copy')
          }
          title={
            copied
              ? t('devices.mobileSync.credential.copied')
              : t('devices.mobileSync.credential.copy')
          }
          onClick={handleCopy}
        >
          {copied ? (
            <Check className="h-3.5 w-3.5 text-emerald-500" />
          ) : (
            <Copy className="h-3.5 w-3.5" />
          )}
        </Button>
      </div>
    </div>
  )
}

interface ScanStepperProps {
  currentStep: ScanStep
  onSelect: (step: ScanStep) => void
}

/**
 * 极简两步导航 —— "① 下载 · ② 配对",一行文字。
 *
 * 设计上故意没有圆 badge + 连接线: 只有两步, 视觉装饰反而抢 QR 的焦点;
 * 编号 + 标签 + 中点分隔已经足够传达"我在第几步"。
 * 完成步(currentStep 之前)是 muted + clickable(可点回退); 当前步是
 * foreground 加粗。点击规则: 只能往回点, 不能往前点 — 强制走"下一步"
 * 按钮, 让用户对"我装好了"做明确确认。
 */
const ScanStepper: React.FC<ScanStepperProps> = ({ currentStep, onSelect }) => {
  const { t } = useTranslation()
  const steps: { id: ScanStep; labelKey: string }[] = [
    { id: 'download', labelKey: 'devices.mobileSync.credential.scan.stepper.download' },
    { id: 'pair', labelKey: 'devices.mobileSync.credential.scan.stepper.pair' },
  ]
  const currentIndex = steps.findIndex(s => s.id === currentStep)

  return (
    <div className="flex items-center justify-center gap-1.5 text-xs">
      {steps.map((step, index) => {
        const isCurrent = index === currentIndex
        const isPast = index < currentIndex
        const clickable = isPast
        return (
          <React.Fragment key={step.id}>
            <button
              type="button"
              disabled={!clickable}
              onClick={clickable ? () => onSelect(step.id) : undefined}
              className={cn(
                'rounded-sm px-1 py-0.5 transition-colors',
                isCurrent && 'font-medium text-foreground',
                isPast &&
                  'cursor-pointer text-muted-foreground underline-offset-2 hover:text-foreground hover:underline',
                !isCurrent && !isPast && 'cursor-default text-muted-foreground/50'
              )}
              aria-current={isCurrent ? 'step' : undefined}
            >
              {index + 1}. {t(step.labelKey)}
            </button>
            {index < steps.length - 1 && <span className="text-muted-foreground/40">·</span>}
          </React.Fragment>
        )
      })}
    </div>
  )
}

interface QrPanelProps {
  /** QR 框顶端的一行内嵌说明。 */
  hint: string
  /** 右上角的额外操作(如"浏览器打开下载页"图标按钮)。 */
  topRight?: React.ReactNode
  children: React.ReactNode
}

/**
 * QR 展示面板 —— hint + (可选) topRight icon + QR 主体。
 *
 * 把原来的"独立提示卡 + 独立 QR 卡 + 独立兜底链接"压成一个卡, hint 内嵌到
 * 顶端, 兜底操作内嵌到右上角图标位。视觉重心全部落到 QR 上, 减少 7 段
 * 独立文字到 1 段。
 */
const QrPanel: React.FC<QrPanelProps> = ({ hint, topRight, children }) => (
  <div className="flex flex-col items-center gap-3 rounded-md border border-border/60 bg-muted/30 p-4">
    <div className="flex w-full items-center justify-between gap-2">
      <span className="text-xs text-muted-foreground">{hint}</span>
      {topRight}
    </div>
    {children}
  </div>
)

export default MobileSyncCredentialModal
