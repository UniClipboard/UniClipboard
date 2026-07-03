/**
 * MobileSyncInstallHelper —— "还没装客户端?" 折叠区。
 *
 * 展开后是 iOS / Android 二选一 tab, 每个 tab 主操作 = 大 QR 扫码下载对应
 * App (iOS → TestFlight 邀请链接 QR; Android → GitHub Releases APK 页 QR)。
 * 用户在桌面上不需要手动复制 URL, 拿手机对屏一扫即可在浏览器打开下载入口。
 *
 * iOS tab 多一个二级"或安装快捷指令"link, 作为对不愿/不能装 App 的兜底
 * (装一次后任何"扫码接入" QR 都能用)。Android 没有这条兜底 — uc-android
 * 是 SyncClipboard 协议兼容的 fork, 不需要 shortcut。
 *
 * 历史: 原为 MobileSyncCredentialModal 的私有子组件。#--- 退休注册结果
 * modal 后, 该引导下沉到 MobileDevicePanel 的 fresh 态 (刚添加设备), 故
 * 抽成独立可复用组件。安装 QR 仅注册结果携带, 重置密码路径没有它, 因此
 * 本组件只在拿得到 `installQrCodePngBase64` 时渲染。
 */

import { openUrl } from '@tauri-apps/plugin-opener'
import { ChevronDown, ChevronRight, ExternalLink, Smartphone } from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-sync-install-helper')

// 产品级常量 — 不本地化, 直接面向用户。
// iOS App 当前在 TestFlight public beta, 用户必须先装 TestFlight 才能装本
// App。短期内是 iOS 推荐路径。
const TESTFLIGHT_URL = 'https://testflight.apple.com/join/nyNQ8dQe'
// Android 客户端是 SyncClipboard 协议兼容的 fork, APK 走 GitHub releases。
const ANDROID_RELEASES_URL = 'https://github.com/UniClipboard/uc-android/releases/latest'

interface MobileSyncInstallHelperProps {
  /** SyncClipboard 快捷指令的安装 QR (后端渲染 base64 PNG)。 */
  installQrCodePngBase64: string
}

type NoClientTab = 'ios' | 'android'

export const MobileSyncInstallHelper: React.FC<MobileSyncInstallHelperProps> = ({
  installQrCodePngBase64,
}) => {
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
 * 一个 📷 icon 按钮, 点击弹 popover 显示 QR。popover 内 QR 用 192px,
 * 桌面屏对着扫足够; 不需要再大 — 一旦超过 ~240px, popover 自身高度会
 * 顶到容器边界, 看着拥挤。
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

export default MobileSyncInstallHelper
