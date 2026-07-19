/**
 * MobileSyncInstallHelper — the collapsible "Haven't installed a client?" block.
 *
 * Expanded, it's an iOS / Android tab pair; each tab's primary action is a
 * large QR to download the matching app (iOS → TestFlight invite link QR;
 * Android → GitHub Releases APK page QR). The user never copies a URL on the
 * desktop — they point their phone at the screen and the download entry opens
 * in the mobile browser.
 *
 * The iOS tab has a secondary "or install the Shortcut" link as a fallback for
 * users who won't/can't install the app (install it once and every "scan to
 * add" QR works). Android has no such fallback — uc-android is a
 * SyncClipboard-protocol-compatible fork and needs no shortcut.
 *
 * History: originally a private child of MobileSyncCredentialModal. After the
 * post-registration modal was retired (#1291), this helper moved down into
 * MobileDevicePanel's fresh state (just-added device), so it was extracted into
 * a standalone reusable component. The install QR only ships with a
 * registration result; the password-reset path never carries it, so this
 * component only renders when `installQrCodePngBase64` is available.
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

// Product-level constants — not localized, user-facing as-is.
// The iOS app is currently a TestFlight public beta; users must install
// TestFlight first. This is the recommended iOS path for now.
const TESTFLIGHT_URL = 'https://testflight.apple.com/join/nyNQ8dQe'
// The Android client is a SyncClipboard-protocol-compatible fork; the APK
// ships via GitHub releases.
const ANDROID_RELEASES_URL = 'https://github.com/UniClipboard/uc-android/releases/latest'

interface MobileSyncInstallHelperProps {
  /** Install QR for the SyncClipboard Shortcut (backend-rendered base64 PNG). */
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
      <CollapsibleTrigger
        render={
          <button
            type="button"
            className="flex w-full items-center justify-between rounded-md border border-border/60 bg-card px-3 py-2 text-sm hover:bg-accent/50"
          />
        }
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
            {/* Fallback: users who don't want the app take the Shortcut path
                (install once, works for every later scan). Visually a secondary
                link + small QR-icon popover, so it doesn't steal focus from the
                primary app QR. */}
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
 * Shared "scan to download the app" panel — used by both the iOS and Android
 * tabs:
 * - a large centered QR (160px), reachable by a phone camera pointed at the
 *   desktop screen
 * - a caption line explaining what the scan installs
 * - an outline "open in browser" secondary button as a fallback for mouse users
 *   (who can also finish the download by logging into GitHub / Apple ID in the
 *   desktop browser)
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
  /** Backend-prerendered PNG base64 to display. */
  imageSrc: string
  imageAlt: string
}

/**
 * A 📷 icon button that opens a popover showing the QR. The popover QR is 192px
 * — enough to scan off the desktop screen, and no larger: past ~240px the
 * popover's own height bumps into the container edge and looks cramped.
 */
const QrPopoverButton: React.FC<QrPopoverButtonProps> = ({ ariaLabel, imageSrc, imageAlt }) => (
  <Popover>
    <PopoverTrigger
      render={
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={ariaLabel}
          title={ariaLabel}
        />
      }
    >
      <Smartphone className="h-3.5 w-3.5" />
    </PopoverTrigger>
    <PopoverContent className="w-auto p-3" align="end">
      <div className="rounded bg-white p-2">
        <img src={imageSrc} alt={imageAlt} className="h-48 w-48" />
      </div>
    </PopoverContent>
  </Popover>
)

export default MobileSyncInstallHelper
