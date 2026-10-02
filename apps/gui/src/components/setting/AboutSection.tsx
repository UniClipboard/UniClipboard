import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { Loader2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  captureUpdateActionInvoked,
  captureUpdateDialogOpened,
  captureUpdateDismissed,
  type DismissSource,
  toUiPhase,
} from '@/api/update-telemetry'
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
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/toast'
import { PackageManagerUpdateDialog } from '@/components/update/PackageManagerUpdateDialog'
import { UpdateDetails } from '@/components/update/UpdateDetails'
import { useSetting } from '@/hooks/useSetting'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { useUpdate } from '@/hooks/useUpdate'
import { createLogger } from '@/lib/logger'
import appIcon from '@/updater/app-icon.png'
import { SponsorsGroup } from './about/SponsorsGroup'
import { UpdatePreferencesGroup } from './UpdatePreferencesGroup'

const log = createLogger('about-section')

function parseChannel(version: string): string {
  const match = version.match(/-(alpha|beta|rc)/)
  return match ? match[1] : 'stable'
}

function getChannelBadgeVariant(channel: string): 'outline' | 'secondary' {
  return channel === 'stable' ? 'secondary' : 'outline'
}

function getChannelLabel(channel: string): string {
  const labels: Record<string, string> = {
    alpha: 'Alpha',
    beta: 'Beta',
    rc: 'RC',
    stable: 'Stable',
  }
  return labels[channel] ?? channel
}

const handleOpenUpdaterWindowDev = async () => {
  try {
    await invoke('dev_open_updater_window', { trace: null })
  } catch (error) {
    log.error({ err: error }, 'Dev open updater window failed')
    toast.error(String(error))
  }
}

const AboutSection: React.FC = () => {
  const { t } = useTranslation()
  const { loading: settingLoading } = useSetting()
  const {
    updateInfo,
    isCheckingUpdate,
    checkForUpdates,
    installUpdate,
    downloadProgress,
    installKind,
    isManualUpdate,
  } = useUpdate()
  const [appVersion, setAppVersion] = useState<string>('')
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false)
  const [packageManagerDialogOpen, setPackageManagerDialogOpen] = useState(false)
  /** See `Sidebar.tsx` for the dismissal-reason ref pattern. */
  const dialogDismissReasonRef = useRef<DismissSource | null>(null)
  const isInstallingUpdate =
    downloadProgress.phase === 'downloading' || downloadProgress.phase === 'installing'
  useShortcutLayer({
    layer: 'modal',
    scope: 'modal',
    enabled: updateDialogOpen,
  })

  const channel = appVersion ? parseChannel(appVersion) : null

  useEffect(() => {
    let cancelled = false
    getVersion()
      .then(version => {
        if (!cancelled) setAppVersion(version)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, 'Failed to get app version')
      })
    return () => {
      cancelled = true
    }
  }, [])

  const handleCheckUpdate = async () => {
    try {
      const update = await checkForUpdates()
      if (!update) {
        toast.success(t('update.noUpdate'))
        return
      }
      // After a successful check the backend state is at least `available`,
      // so `toUiPhase` returns a non-null value. Fall back to `available`
      // defensively if state hasn't propagated yet.
      const uiPhase = toUiPhase(downloadProgress.phase) ?? 'available'
      captureUpdateDialogOpened('sidebar_icon', uiPhase)
      // deb/rpm: Tauri's in-app updater can't install system packages; route
      // the user to apt/dnf with a copy-able command instead. windowsportable:
      // the NSIS updater would install into Program Files, not the portable
      // folder, so route it to the same "download out-of-band" dialog.
      if (isManualUpdate) {
        setPackageManagerDialogOpen(true)
      } else {
        setUpdateDialogOpen(true)
      }
    } catch (error) {
      log.error({ err: error }, '检查更新失败')
      toast.error(t('update.checkFailed'))
    }
  }

  const handleInstallUpdate = async () => {
    if (!updateInfo || isInstallingUpdate) return
    captureUpdateActionInvoked('install', 'started')
    try {
      await installUpdate()
      setUpdateDialogOpen(false)
    } catch (error) {
      captureUpdateActionInvoked('install', 'failed')
      log.error({ err: error }, '更新失败')
      toast.error(t('update.installFailed'))
    }
  }

  const handleUpdateDialogOpenChange = (open: boolean) => {
    if (!open && updateDialogOpen) {
      const uiPhase = toUiPhase(downloadProgress.phase)
      if (uiPhase) {
        const source: DismissSource = dialogDismissReasonRef.current ?? 'dialog_closed'
        captureUpdateDismissed(uiPhase, source)
      }
      dialogDismissReasonRef.current = null
    }
    setUpdateDialogOpen(open)
  }

  const handlePackageManagerDialogOpenChange = (open: boolean) => {
    if (!open && packageManagerDialogOpen) {
      const uiPhase = toUiPhase(downloadProgress.phase) ?? 'available'
      captureUpdateDismissed(uiPhase, 'package_manager_dialog_closed')
    }
    setPackageManagerDialogOpen(open)
  }

  return (
    <div className="flex min-w-0 flex-col gap-8">
      <div className="flex min-w-0 flex-wrap items-center gap-4 px-1">
        <img src={appIcon} alt="" className="size-12 shrink-0 rounded-lg" />
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-ui-section font-semibold">
              {t('settings.sections.about.appName')}
            </h2>
            {channel && (
              <Badge variant={getChannelBadgeVariant(channel)}>{getChannelLabel(channel)}</Badge>
            )}
          </div>
          <p className="text-ui-caption text-muted-foreground">
            {appVersion
              ? t('settings.sections.about.version', { version: appVersion })
              : t('settings.sections.about.version', { version: '...' })}
          </p>
        </div>
        <Button
          size="sm"
          className="ml-auto w-40 max-w-full transition-colors"
          onClick={handleCheckUpdate}
          disabled={settingLoading || isCheckingUpdate}
          aria-busy={isCheckingUpdate}
        >
          <span className="inline-flex min-w-0 items-center justify-center gap-1.5">
            {isCheckingUpdate && <Loader2 data-icon="inline-start" className="animate-spin" />}
            <span>
              {isCheckingUpdate
                ? t('settings.sections.about.checkingUpdate')
                : t('settings.sections.about.checkUpdate')}
            </span>
          </span>
        </Button>
        {import.meta.env.DEV && (
          <button
            type="button"
            className="w-full text-left text-ui-body text-amber-600 underline-offset-2 hover:underline dark:text-amber-400"
            onClick={handleOpenUpdaterWindowDev}
            title="Dev only: open the Sparkle-style updater window with mock data"
          >
            Open updater window (dev)
          </button>
        )}
      </div>

      {/* Update settings */}
      <UpdatePreferencesGroup />

      {/* Sponsors */}
      <SponsorsGroup />

      {/* Footer: links + copyright */}
      <div className="space-y-2.5 pt-1 text-center">
        <div className="flex justify-center gap-x-5 text-ui-body">
          <a
            href="https://github.com/UniClipboard/UniClipboard"
            className="text-muted-foreground transition-colors hover:text-foreground"
            target="_blank"
            rel="noreferrer"
          >
            {t('settings.sections.about.links.privacyPolicy')}
          </a>
          <a
            href="https://github.com/UniClipboard/UniClipboard"
            className="text-muted-foreground transition-colors hover:text-foreground"
            target="_blank"
            rel="noreferrer"
          >
            {t('settings.sections.about.links.termsOfService')}
          </a>
          <a
            href="https://github.com/UniClipboard/UniClipboard/blob/main/ABOUT.md"
            className="text-muted-foreground transition-colors hover:text-foreground"
            target="_blank"
            rel="noreferrer"
          >
            {t('settings.sections.about.links.aboutMaintainers')}
          </a>
        </div>
        <p className="text-ui-caption text-muted-foreground/80">
          {t('settings.sections.about.copyright')}
        </p>
      </div>

      <AlertDialog open={updateDialogOpen} onOpenChange={handleUpdateDialogOpenChange}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('update.title')}</AlertDialogTitle>
            <AlertDialogDescription render={<div />} className="space-y-3">
              <UpdateDetails
                currentVersion={updateInfo?.currentVersion}
                version={updateInfo?.version}
                body={updateInfo?.body}
                phase={downloadProgress.phase}
                percent={
                  downloadProgress.total && downloadProgress.total > 0
                    ? (downloadProgress.downloaded / downloadProgress.total) * 100
                    : null
                }
              />
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={isInstallingUpdate}
              onClick={() => {
                dialogDismissReasonRef.current = 'dialog_later'
              }}
            >
              {t('update.later')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={event => {
                event.preventDefault()
                handleInstallUpdate()
              }}
              disabled={isInstallingUpdate}
            >
              {t('update.updateNow')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {installKind && (
        <PackageManagerUpdateDialog
          open={packageManagerDialogOpen}
          onOpenChange={handlePackageManagerDialogOpenChange}
          installKind={installKind}
          updateInfo={updateInfo}
        />
      )}
    </div>
  )
}

export default AboutSection
