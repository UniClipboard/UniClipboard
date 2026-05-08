/**
 * MobileSyncCredentialModal —— 注册成功后展示一次性凭据。
 *
 * 关键不变量(对应 facade 合约,见 RegisterMobileShortcutDeviceOutput 注释):
 * 1. password 字段是**唯一一次**面向用户的明文回显;关闭后服务端只剩 PHC,
 *    无法再取回。
 * 2. UI 必须强制用户勾选「我已保存」才允许关闭(防误关)。
 * 3. password 永远不进 log / 持久化 / analytics(已在 invokeWithTrace 的
 *    sensitive args redaction 处约束;本组件再多一层"卸载即丢"的内存策略,
 *    上层不应把这份对象长期持有)。
 *
 * 平台 tab:LAN 监听跑的是 SyncClipboard 协议,凭据本身平台无关 —— 同一组
 * (base URL + username + password) 在 iOS / Android 客户端上都能用。tab 仅
 * 控制「接入步骤」的展示形式:
 * - iOS(默认):展示 iCloud 快捷指令的安装二维码 + install URL
 * - Android:暂不提供官方客户端,仅展示凭据 + 一段说明,告知用户使用任何
 *   兼容 SyncClipboard 协议的客户端接入
 *
 * 桌面端打开 iCloud 共享链接无意义,不提供 "Open in Shortcuts" 按钮。
 */

import { Check, Copy, Eye, EyeOff } from 'lucide-react'
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
import { cn } from '@/lib/utils'

interface Props {
  /**
   * 凭据 payload。`null` 表示 modal 关闭(等价 open=false)。父组件清空
   * payload 即关闭 modal,本组件不持有任何引用。
   */
  payload: RegisterMobileDeviceResult | null
  onClose: () => void
}

type Platform = 'ios' | 'android'

const MobileSyncCredentialModal: React.FC<Props> = ({ payload, onClose }) => {
  const { t } = useTranslation()
  const [acknowledged, setAcknowledged] = useState(false)
  const [passwordVisible, setPasswordVisible] = useState(false)
  const [platform, setPlatform] = useState<Platform>('ios')
  // 用户尝试关闭但未勾选时的 inline 提示。toast 在 modal 遮罩下很容易被忽视,
  // 改成把红色高亮 + 错误文本直接挂在勾选框上,视线一定会被引到下一步操作。
  const [hintActive, setHintActive] = useState(false)
  const acknowledgeRef = useRef<HTMLLabelElement>(null)

  // 关闭时显式重置内部状态;父组件只控 payload 出现/消失,本组件确保
  // 下一次打开是干净的"未确认 / 密码隐藏 / iOS tab"初始态
  const handleClose = useCallback(() => {
    setAcknowledged(false)
    setPasswordVisible(false)
    setPlatform('ios')
    setHintActive(false)
    onClose()
  }, [onClose])

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
    handleClose()
  }, [acknowledged, flagUnacknowledged, handleClose])

  if (!payload) return null

  return (
    <Dialog
      open
      onOpenChange={open => {
        if (!open) tryClose()
      }}
    >
      <DialogContent
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
        <DialogHeader className="px-4 pt-4 pb-2">
          <DialogTitle>{t('devices.mobileSync.credential.title')}</DialogTitle>
          <DialogDescription>{t('devices.mobileSync.credential.subtitle')}</DialogDescription>
        </DialogHeader>

        <div className="flex-1 space-y-4 overflow-y-auto px-4 py-2">
          {/* 警告横幅 */}
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
            <p className="text-sm font-semibold text-amber-700 dark:text-amber-400">
              {t('devices.mobileSync.credential.warning.title')}
            </p>
            <p className="mt-1 text-xs text-amber-700/90 dark:text-amber-400/90">
              {t('devices.mobileSync.credential.warning.body')}
            </p>
          </div>

          {/* 平台 tab —— 凭据共用,只切换接入步骤的展示 */}
          <Tabs value={platform} onValueChange={v => setPlatform(v as Platform)}>
            <TabsList className="w-full">
              <TabsTrigger value="ios">
                {t('devices.mobileSync.credential.platforms.ios')}
              </TabsTrigger>
              <TabsTrigger value="android">
                {t('devices.mobileSync.credential.platforms.android')}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="ios" className="mt-3 space-y-4">
              {/* 二维码 */}
              <div className="flex flex-col items-center gap-2 rounded-md border border-border/60 bg-muted/30 p-4">
                <Label className="text-xs uppercase tracking-wider text-muted-foreground">
                  {t('devices.mobileSync.credential.qr.label')}
                </Label>
                <img
                  src={`data:image/png;base64,${payload.qrCodePngBase64}`}
                  alt={t('devices.mobileSync.credential.qr.alt')}
                  className="h-48 w-48 rounded bg-white p-2"
                />
              </div>

              {/* Install URL */}
              <CredentialField
                label={t('devices.mobileSync.credential.installUrl.label')}
                value={payload.installUrl}
                mono
              />
            </TabsContent>

            <TabsContent value="android" className="mt-3 space-y-3">
              <div className="rounded-md border border-border/60 bg-muted/30 p-3 text-xs leading-relaxed text-muted-foreground">
                {t('devices.mobileSync.credential.android.instructions')}
              </div>
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
              'flex cursor-pointer items-start gap-2 rounded-md border bg-card p-3 transition-all',
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
    <div className="space-y-1">
      <Label className="text-xs uppercase tracking-wider text-muted-foreground">{label}</Label>
      <div className="flex items-center gap-1 rounded-md border border-border/60 bg-card px-3 py-2">
        {/* min-w-0 is required: flex items default to min-width:auto which prevents
            truncate from shrinking below the intrinsic content width. Without it
            long URLs / passwords push the row past the modal edge. */}
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

export default MobileSyncCredentialModal
