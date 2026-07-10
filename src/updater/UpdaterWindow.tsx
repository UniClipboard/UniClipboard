import { getCurrentWindow } from '@tauri-apps/api/window'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Loader2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  cancelDownload,
  checkForUpdate,
  downloadUpdate,
  getAutoDownloadUpdate,
  getDownloadProgress,
  getInstallKind,
  installUpdate,
  setAutoDownloadUpdate,
  skipVersion,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
  type DownloadEvent,
  type DownloadPhase,
  type DownloadProgressSnapshot,
  type UpdateMetadata,
} from '@/api/updater'
import { Progress } from '@/components/ui/progress'
import { Switch } from '@/components/ui/switch'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'
import { useThemeSync } from '@/hooks/useThemeSync'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import appIcon from '@/updater/app-icon.png'

const log = createLogger('updater-window')

/** Same target as PackageManagerUpdateDialog's manual-download routing. */
const RELEASE_PAGE_URL = 'https://uniclipboard.app/download'

interface UpdateState {
  phase: DownloadPhase
  info: UpdateMetadata | null
  downloaded: number
  total: number | null
  autoUpdate: boolean
}

const initialState: UpdateState = {
  phase: 'idle',
  info: null,
  downloaded: 0,
  total: null,
  autoUpdate: true,
}

const DEV_MOCK: UpdateState = {
  phase: 'available',
  info: {
    version: '0.99.0-dev',
    currentVersion: '0.12.0-alpha.1',
    date: new Date().toISOString(),
    body: "### What's new\n\n- Auto-update prompt now shows the changelog\n- Fixed a few sync edge cases\n- Performance improvements",
  },
  downloaded: 0,
  total: null,
  autoUpdate: true,
}

// Map a backend progress snapshot onto the local state, including `info`.
// Shared by the mount-time sync and the post-failure re-sync so both leave
// the window in the same shape — e.g. a snapshot with no version (pending
// update cleared) must also clear `info`, or the up-to-date view never shows.
const applySnapshot = (prev: UpdateState, s: DownloadProgressSnapshot): UpdateState => ({
  ...prev,
  phase: s.phase,
  info: s.version
    ? { version: s.version, currentVersion: s.currentVersion, body: s.body, date: s.date }
    : null,
  downloaded: s.downloaded,
  total: s.total,
})

const isDevPreview = (): boolean => {
  if (typeof window === 'undefined') return false
  const params = new URLSearchParams(window.location.search)
  return params.get('dev') === '1'
}

function useUpdaterState(devPreview: boolean) {
  const [state, setState] = useState<UpdateState>(() => (devPreview ? DEV_MOCK : initialState))
  const [cancelling, setCancelling] = useState(false)
  // Re-checking the latest version before committing to install (see
  // `handleInstall`). Kept separate from `phase` so the button can show a
  // transient loading state without entering the download state machine.
  const [preparing, setPreparing] = useState(false)
  // Windows portable ("green") zip cannot self-install: the NSIS payload would
  // install into Program Files instead of refreshing the portable folder. The
  // scheduler already skips auto-download for it, but this window previously
  // still offered "Install Update" — clicking it failed and read as "updates
  // are broken" (observed in the field). Detect the kind and route portable
  // users to a manual download instead.
  const [isPortable, setIsPortable] = useState(false)

  useEffect(() => {
    if (devPreview) return
    let cancelled = false
    void getInstallKind()
      .then(kind => {
        if (!cancelled) setIsPortable(kind === 'windowsportable')
      })
      .catch(err => log.warn({ err }, '获取安装类型失败；按可自更新处理'))
    return () => {
      cancelled = true
    }
  }, [devPreview])

  useEffect(() => {
    if (devPreview) return
    let cancelled = false
    void Promise.allSettled([getDownloadProgress(), getAutoDownloadUpdate()]).then(
      ([progressResult, autoUpdateResult]) => {
        if (cancelled) return
        setState(prev => {
          let next = prev
          if (progressResult.status === 'fulfilled') {
            next = applySnapshot(next, progressResult.value)
          }
          if (autoUpdateResult.status === 'fulfilled') {
            next = { ...next, autoUpdate: autoUpdateResult.value }
          }
          return next
        })
        if (progressResult.status === 'rejected') {
          log.error({ err: progressResult.reason }, '获取下载状态失败')
        }
        if (autoUpdateResult.status === 'rejected') {
          log.error({ err: autoUpdateResult.reason }, '获取自动下载设置失败')
        }
      }
    )
    return () => {
      cancelled = true
    }
  }, [devPreview])

  useEffect(() => {
    if (devPreview) return
    let cancelled = false
    let unlistenAvailable: (() => void) | undefined
    let unlistenProgress: (() => void) | undefined

    void subscribeUpdateAvailable(meta => {
      if (!meta) return
      setState(prev => {
        if (prev.info && prev.info.version !== meta.version) {
          return { ...prev, phase: 'available', info: meta, downloaded: 0, total: null }
        }
        return {
          ...prev,
          phase: prev.phase === 'idle' ? 'available' : prev.phase,
          info: meta,
        }
      })
    })
      .then(fn => {
        if (cancelled) fn()
        else unlistenAvailable = fn
      })
      .catch(err => log.error({ err }, '订阅 update-available 失败'))

    void subscribeUpdateProgress((event: DownloadEvent) => {
      setState(prev => {
        switch (event.event) {
          case 'Started':
            return { ...prev, phase: 'downloading', downloaded: 0, total: event.data.contentLength }
          case 'Progress':
            return { ...prev, downloaded: prev.downloaded + event.data.chunkLength }
          case 'Finished':
            return { ...prev, phase: 'ready', total: prev.total ?? prev.downloaded }
          case 'Failed':
            return { ...prev, phase: prev.info ? 'available' : 'idle', downloaded: 0, total: null }
        }
      })
    })
      .then(fn => {
        if (cancelled) fn()
        else unlistenProgress = fn
      })
      .catch(err => log.error({ err }, '订阅 update-progress 失败'))

    return () => {
      cancelled = true
      unlistenAvailable?.()
      unlistenProgress?.()
    }
  }, [devPreview])

  const closeWindow = useCallback(() => {
    getCurrentWindow()
      .close()
      .catch(err => log.error({ err }, '关闭 updater 窗口失败'))
  }, [])

  const handleSkip = useCallback(async () => {
    if (!state.info) {
      closeWindow()
      return
    }
    try {
      await skipVersion(state.info.version)
      closeWindow()
    } catch (err) {
      log.error({ err }, '跳过版本失败')
    }
  }, [state.info, closeWindow])

  const handleAutoUpdateToggle = useCallback(
    (checked: boolean) => {
      setState(prev => ({ ...prev, autoUpdate: checked }))
      if (!devPreview) {
        void setAutoDownloadUpdate(checked).catch(err => {
          setState(prev => ({ ...prev, autoUpdate: !checked }))
          log.error({ err }, '设置自动下载失败')
        })
      }
    },
    [devPreview]
  )

  // Start (or resume) the download through the recoverable background path.
  // Unlike the legacy inline `download_and_install`, this writes progress into
  // the backend's shared `PendingUpdate` state, so closing the window mid-
  // download leaves the download running and re-openable — the whole point of
  // the "download in background" affordance. Progress and the `downloading ->
  // ready` transition arrive via the `subscribeUpdateProgress` broadcast that
  // this window already listens to, so no per-call progress callback is needed.
  const handleDownload = useCallback(async () => {
    if (devPreview) {
      setState(prev => ({ ...prev, phase: 'downloading', downloaded: 0, total: 100 }))
      let bytes = 0
      const id = window.setInterval(() => {
        bytes = Math.min(100, bytes + 20)
        setState(prev => ({ ...prev, downloaded: bytes }))
        if (bytes >= 100) {
          window.clearInterval(id)
          setState(prev => ({ ...prev, phase: 'ready' }))
        }
      }, 250)
      return
    }
    // Defense in depth: the portable UI replaces this button with the
    // release-page action, but never let a portable build reach the NSIS
    // installer even if the kind probe raced the click.
    if (isPortable) {
      openUrl(RELEASE_PAGE_URL).catch(err => log.error({ err }, '打开发布页失败'))
      return
    }

    // Re-check the latest version before committing to download. The pending
    // update is a snapshot from when the scheduler first detected it; if a
    // newer release shipped since (e.g. 0.14.0 popup while 0.14.1 is out),
    // downloading the cached version lands the user on an already-outdated
    // build that immediately prompts again. A fresh check lets the backend
    // supersede the pending state and re-emit `update-available`, which
    // refreshes this window to the newer version + changelog so the user can
    // review and confirm with a second click.
    const pendingVersion = state.info?.version
    if (pendingVersion) {
      setPreparing(true)
      try {
        const latest = await checkForUpdate()
        if (!latest) {
          // No longer offered (e.g. release pulled). The backend has cleared
          // the pending state; reflect up-to-date instead of downloading a
          // version that no longer exists.
          setPreparing(false)
          setState(prev => ({ ...prev, phase: 'idle', info: null }))
          return
        }
        if (latest.version !== pendingVersion) {
          // A different (newer) version is available. The window has already
          // been refreshed via the `update-available` broadcast; stop so the
          // user confirms the new version explicitly.
          setPreparing(false)
          return
        }
      } catch (err) {
        // Offline / timeout: fall through and download the cached version
        // rather than blocking the update on a failed re-check.
        log.warn({ err }, '下载前重新检查失败；下载已缓存版本')
      }
      setPreparing(false)
    }

    // Optimistically enter the downloading state so the button flips to the
    // cancel/background pair immediately; the `Started` broadcast will confirm
    // and supply the content length.
    setState(prev => ({ ...prev, phase: 'downloading', downloaded: 0, total: null }))
    try {
      await downloadUpdate()
    } catch (error) {
      // A `Failed` broadcast already reset the phase for a real download
      // failure/cancellation. Precondition rejections (e.g. a concurrent
      // scheduler download already in flight) emit no broadcast, so re-sync
      // the phase from the backend rather than leaving a stale spinner.
      log.error({ err: error }, '后台下载更新失败')
      getDownloadProgress()
        .then(s => {
          setState(prev => applySnapshot(prev, s))
        })
        .catch(err => {
          log.error({ err }, '下载失败后同步进度失败')
          setState(prev => ({ ...prev, phase: prev.info ? 'available' : 'idle' }))
        })
    }
  }, [devPreview, isPortable, state.info])

  // Install the already-downloaded bytes and restart. Reached only from the
  // `ready` phase, so the backend takes the cached-bytes install path — no
  // second download happens here.
  const handleInstall = useCallback(async () => {
    if (devPreview) {
      setState(prev => ({ ...prev, phase: 'installing' }))
      return
    }
    if (isPortable) {
      openUrl(RELEASE_PAGE_URL).catch(err => log.error({ err }, '打开发布页失败'))
      return
    }
    try {
      await installUpdate(progress => {
        setState(prev => ({
          ...prev,
          phase: progress.phase === 'installing' ? 'installing' : 'downloading',
          downloaded: progress.downloaded,
          total: progress.total,
        }))
      })
    } catch (error) {
      log.error({ err: error }, '安装更新失败')
      setState(prev => ({ ...prev, phase: prev.info ? 'available' : 'idle' }))
    }
  }, [devPreview, isPortable])

  const handleCancel = useCallback(async () => {
    if (devPreview || cancelling) return
    setCancelling(true)
    try {
      await cancelDownload()
    } catch (error) {
      log.error({ err: error }, '取消下载失败')
    } finally {
      setCancelling(false)
    }
  }, [devPreview, cancelling])

  return {
    state,
    cancelling,
    preparing,
    isPortable,
    closeWindow,
    handleSkip,
    handleAutoUpdateToggle,
    handleDownload,
    handleInstall,
    handleCancel,
  }
}

const ActionButtons: React.FC<{
  phase: DownloadPhase
  hasInfo: boolean
  cancelling: boolean
  /** Re-checking the latest version before download; disables the actions. */
  preparing: boolean
  /** Portable build: primary action opens the release page instead of installing. */
  isPortable: boolean
  onCancel: () => void
  onSkip: () => void
  onClose: () => void
  onDownload: () => void
  onInstall: () => void
}> = ({
  phase,
  hasInfo,
  cancelling,
  preparing,
  isPortable,
  onCancel,
  onSkip,
  onClose,
  onDownload,
  onInstall,
}) => {
  const { t } = useTranslation()
  const isDownloading = phase === 'downloading'
  const isInstalling = phase === 'installing'
  const isReady = phase === 'ready'
  const upToDate = phase === 'idle' && !hasInfo

  if (isDownloading) {
    return (
      <>
        <div className="flex-1" />
        <button
          type="button"
          className="mr-2 rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
          onClick={onCancel}
          disabled={cancelling}
        >
          {cancelling ? t('update.cancelling') : t('update.cancelDownload')}
        </button>
        {/* "Download in background" just dismisses the window — the download
            already runs in the backend, and re-opening restores progress via
            the mount-time `getDownloadProgress` sync. */}
        <button
          type="button"
          className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          onClick={onClose}
        >
          {t('update.downloadInBackground')}
        </button>
      </>
    )
  }

  if (isInstalling) {
    return (
      <>
        <div className="flex-1" />
        <button
          type="button"
          className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground opacity-60"
          disabled
        >
          <Loader2 className="size-4 animate-spin" />
          {t('update.installing')}
        </button>
      </>
    )
  }

  if (upToDate) {
    return (
      <>
        <div className="flex-1" />
        <button
          type="button"
          className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
          onClick={onClose}
        >
          {t('updater.window.close')}
        </button>
      </>
    )
  }

  return (
    <>
      <button
        type="button"
        className="rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
        onClick={onSkip}
        disabled={preparing}
      >
        {t('updater.window.skipThisVersion')}
      </button>
      <div className="flex-1" />
      <button
        type="button"
        className="mr-2 rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
        onClick={onClose}
        disabled={preparing}
      >
        {t('updater.window.remindMeLater')}
      </button>
      <button
        type="button"
        className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
        onClick={isReady ? onInstall : onDownload}
        disabled={!hasInfo || preparing}
      >
        {preparing ? (
          <>
            <Loader2 className="size-4 animate-spin" />
            {t('updater.window.checking')}
          </>
        ) : isPortable ? (
          t('update.packageManager.openReleasePage')
        ) : isReady ? (
          t('update.installNow')
        ) : (
          t('updater.window.downloadUpdate')
        )}
      </button>
    </>
  )
}

const UpdaterWindow: React.FC = () => {
  useThemeSync()

  const { t } = useTranslation()
  const devPreview = isDevPreview()
  const {
    state,
    cancelling,
    preparing,
    isPortable,
    closeWindow,
    handleSkip,
    handleAutoUpdateToggle,
    handleDownload,
    handleInstall,
    handleCancel,
  } = useUpdaterState(devPreview)

  const { phase, info, downloaded, total, autoUpdate } = state
  const percent = total !== null && total > 0 ? Math.round((downloaded / total) * 100) : null
  const busy = phase === 'downloading' || phase === 'installing'
  const upToDate = phase === 'idle' && !info

  const headline = upToDate ? t('updater.window.upToDateTitle') : t('updater.window.title')

  const subtitle = upToDate
    ? t('updater.window.upToDateBody')
    : phase === 'ready' && info
      ? t('updater.window.readySubtitle', { app: 'UniClipboard', version: info.version })
      : info
        ? t('updater.window.subtitle', {
            app: 'UniClipboard',
            version: info.version,
            currentVersion: info.currentVersion ?? '?',
          })
        : ''

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex gap-4 px-6 pt-5">
        <img src={appIcon} alt="" className="size-12 shrink-0 rounded-xl" draggable={false} />
        <div className="flex min-w-0 flex-col gap-0.5">
          <h1 className="text-[15px] font-bold leading-tight">{headline}</h1>
          {subtitle && <p className="text-[13px] leading-snug text-muted-foreground">{subtitle}</p>}
        </div>
      </div>

      {busy && (
        <div className="mx-6 mt-4 space-y-1.5">
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{phase === 'installing' ? t('update.installing') : t('update.downloading')}</span>
            {percent !== null && <span>{percent}%</span>}
          </div>
          <Progress
            value={percent ?? undefined}
            className={cn('h-2', percent === null && 'animate-pulse')}
          />
        </div>
      )}

      {/* Release notes for the detected version. The auto-detect path opens
          this window directly (no main window), so — like the in-app About
          dialog — it must surface the changelog here; otherwise the user has
          to visit GitHub to see what changed (issue #1268). */}
      {!upToDate && info && (
        <div className="mx-6 mt-4 flex min-h-0 flex-1 flex-col gap-1.5">
          <div className="shrink-0 text-[13px] font-medium text-foreground">
            {t('update.releaseNotes')}
          </div>
          <div className="scrollbar-thin min-h-0 flex-1 overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
            <ReleaseNotes content={info.body ?? ''} fallback={t('update.noNotes')} />
          </div>
        </div>
      )}

      {/* Portable build: explain the manual download+replace flow instead of
          the auto-update toggle — in-place self-update does not apply, and a
          silent install failure here previously read as "updates are broken". */}
      {!busy && !upToDate && isPortable && (
        <p className="mx-6 mt-4 text-[13px] leading-snug text-muted-foreground">
          {t('update.packageManager.portableHint')}
        </p>
      )}

      {!busy && !upToDate && !isPortable && (
        <label className="mx-6 mt-4 flex cursor-pointer items-center gap-2.5">
          <Switch size="sm" checked={autoUpdate} onCheckedChange={handleAutoUpdateToggle} />
          <span className="text-[13px] text-muted-foreground">
            {t('updater.window.autoUpdate')}
          </span>
        </label>
      )}

      <div className="mt-auto flex items-center px-6 py-4">
        <ActionButtons
          phase={phase}
          hasInfo={!!info}
          cancelling={cancelling}
          preparing={preparing}
          isPortable={isPortable}
          onCancel={() => void handleCancel()}
          onSkip={handleSkip}
          onClose={closeWindow}
          onDownload={() => void handleDownload()}
          onInstall={() => void handleInstall()}
        />
      </div>
    </div>
  )
}

export default UpdaterWindow
