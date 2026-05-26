import { getCurrentWindow } from '@tauri-apps/api/window'
import { Download, Loader2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  cancelDownload,
  getDownloadProgress,
  installUpdate,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
  type DownloadEvent,
  type DownloadPhase,
  type UpdateMetadata,
} from '@/api/updater'
import { Progress } from '@/components/ui/progress'
import { ReleaseNotes } from '@/components/update/ReleaseNotes'
import { useThemeSync } from '@/hooks/useThemeSync'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('updater-window')

interface UpdateState {
  phase: DownloadPhase
  info: UpdateMetadata | null
  downloaded: number
  total: number | null
}

const initialState: UpdateState = {
  phase: 'idle',
  info: null,
  downloaded: 0,
  total: null,
}

/** Hardcoded mock used by the dev-only "Open Updater Window" entry. */
const DEV_MOCK: UpdateState = {
  phase: 'available',
  info: {
    version: '0.99.0-dev',
    currentVersion: '0.12.0-alpha.1',
    date: new Date().toISOString(),
    body: [
      "## What's new",
      '',
      '- Sparkle-style updater window (dev preview)',
      '- Release notes render via markdown',
      '- 立即更新 / 稍后 actions wire to the real backend',
      '',
      '## Fixes',
      '',
      '- Sidebar update indicator was too easy to miss',
    ].join('\n'),
  },
  downloaded: 0,
  total: null,
}

const isDevPreview = (): boolean => {
  if (typeof window === 'undefined') return false
  const params = new URLSearchParams(window.location.search)
  return params.get('dev') === '1'
}

const UpdaterWindow: React.FC = () => {
  // Independent webview — no SettingContext here, so pull theme straight from
  // the daemon (same approach as the quick panel). Falls back to system
  // prefers-color-scheme if settings load fails (e.g. dev preview without
  // daemon).
  useThemeSync()

  const { t } = useTranslation()
  const [state, setState] = useState<UpdateState>(initialState)
  const [cancelling, setCancelling] = useState(false)
  const devPreview = isDevPreview()

  // Hydrate state: dev preview uses the mock; real path syncs from backend.
  useEffect(() => {
    if (devPreview) {
      setState(DEV_MOCK)
      return
    }
    let cancelled = false
    void getDownloadProgress()
      .then(snapshot => {
        if (cancelled) return
        setState({
          phase: snapshot.phase,
          info: snapshot.version
            ? {
                version: snapshot.version,
                currentVersion: snapshot.currentVersion,
                body: snapshot.body,
                date: snapshot.date,
              }
            : null,
          downloaded: snapshot.downloaded,
          total: snapshot.total,
        })
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, '获取下载状态失败')
      })
    return () => {
      cancelled = true
    }
  }, [devPreview])

  // Live broadcasts: keep the window in sync if scheduler emits new state
  // while it's open (rare but possible — e.g. a second check finishes).
  useEffect(() => {
    if (devPreview) return
    let cancelled = false
    let unlistenAvailable: (() => void) | undefined
    let unlistenProgress: (() => void) | undefined

    void subscribeUpdateAvailable(meta => {
      if (!meta) return
      setState(prev => {
        // A new version supersedes any in-flight or already-downloaded artifact
        // tied to the previous version — otherwise the UI would offer "Install"
        // for meta.version while the bytes on disk are still the old build.
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

  const handleInstall = useCallback(async () => {
    if (devPreview) {
      // Dev preview must not trigger a real install — just fake a quick
      // downloading → ready transition so the UI states stay reachable.
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
  }, [devPreview])

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

  const { phase, info, downloaded, total } = state
  const percent = total !== null && total > 0 ? Math.round((downloaded / total) * 100) : null
  const isDownloading = phase === 'downloading'
  const isInstalling = phase === 'installing'
  const isReady = phase === 'ready'
  const busy = isDownloading || isInstalling
  const upToDate = phase === 'idle' && !info

  // Sparkle convention: title carries the headline; subtitle mentions versions.
  const headline = upToDate ? t('updater.window.upToDateTitle') : t('updater.window.title')
  const subtitle = upToDate
    ? t('updater.window.upToDateBody')
    : info
      ? t('updater.window.subtitle', { app: 'UniClipboard', version: info.version })
      : ''

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden rounded-xl border border-border/50 bg-background text-foreground shadow-2xl">
      <div data-tauri-drag-region className="flex items-start gap-4 px-6 pt-6">
        <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-primary to-primary/60 text-primary-foreground shadow-lg shadow-primary/20">
          <Download className="h-7 w-7" />
        </div>
        <div className="flex-1 space-y-1">
          <h1 className="text-base font-semibold leading-tight">{headline}</h1>
          {subtitle && <p className="text-sm text-muted-foreground">{subtitle}</p>}
        </div>
      </div>

      {!upToDate && info && (
        <>
          <div className="mx-6 mt-4 grid grid-cols-2 gap-2 rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-sm">
            <div className="space-y-0.5">
              <div className="text-xs uppercase tracking-wide text-muted-foreground">
                {t('updater.window.currentVersion')}
              </div>
              <div className="font-medium">{info.currentVersion ?? '-'}</div>
            </div>
            <div className="space-y-0.5">
              <div className="text-xs uppercase tracking-wide text-muted-foreground">
                {t('updater.window.latestVersion')}
              </div>
              <div className="font-medium text-primary">{info.version}</div>
            </div>
          </div>

          <div className="mx-6 mt-4 flex-1 overflow-hidden">
            <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t('updater.window.releaseNotes')}
            </div>
            <div className="mt-1 h-full overflow-auto rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
              <ReleaseNotes content={info.body ?? ''} fallback={t('update.noNotes')} />
            </div>
          </div>
        </>
      )}

      {busy && (
        <div className="mx-6 mt-3 space-y-1.5">
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{isInstalling ? t('update.installing') : t('update.downloading')}</span>
            {percent !== null && <span>{percent}%</span>}
          </div>
          <Progress
            value={percent ?? undefined}
            className={cn('h-2', percent === null && 'animate-pulse')}
          />
        </div>
      )}

      <div className="flex items-center justify-end gap-2 border-t border-border/60 px-6 py-3 mt-4">
        {isDownloading ? (
          <>
            <button
              type="button"
              className="rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
              onClick={() => void handleCancel()}
              disabled={cancelling}
            >
              {cancelling ? t('update.cancelling') : t('update.cancelDownload')}
            </button>
            <button
              type="button"
              className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground opacity-60"
              disabled
            >
              <Loader2 className="h-4 w-4 animate-spin" />
              {t('update.downloading')}
            </button>
          </>
        ) : upToDate ? (
          <button
            type="button"
            className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
            onClick={closeWindow}
          >
            {t('updater.window.close')}
          </button>
        ) : (
          <>
            <button
              type="button"
              className="rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
              onClick={closeWindow}
              disabled={isInstalling}
            >
              {t('update.later')}
            </button>
            <button
              type="button"
              className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              onClick={() => void handleInstall()}
              disabled={isInstalling || (!devPreview && !info)}
            >
              {isReady ? t('update.installNow') : t('update.updateNow')}
            </button>
          </>
        )}
      </div>
    </div>
  )
}

export default UpdaterWindow
