import { getCurrentWindow } from '@tauri-apps/api/window'
import { Loader2 } from 'lucide-react'
import React, { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  cancelDownload,
  getAutoDownloadUpdate,
  getDownloadProgress,
  installUpdate,
  setAutoDownloadUpdate,
  skipVersion,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
  type DownloadEvent,
  type DownloadPhase,
  type UpdateMetadata,
} from '@/api/updater'
import { Progress } from '@/components/ui/progress'
import { Switch } from '@/components/ui/switch'
import { useThemeSync } from '@/hooks/useThemeSync'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import appIcon from '@/updater/app-icon.png'

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

const DEV_MOCK: UpdateState = {
  phase: 'available',
  info: {
    version: '0.99.0-dev',
    currentVersion: '0.12.0-alpha.1',
    date: new Date().toISOString(),
    body: '',
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
  useThemeSync()

  const { t } = useTranslation()
  const [state, setState] = useState<UpdateState>(initialState)
  const [cancelling, setCancelling] = useState(false)
  const [autoUpdate, setAutoUpdate] = useState(true)
  const devPreview = isDevPreview()

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
    void getAutoDownloadUpdate()
      .then(enabled => {
        if (!cancelled) setAutoUpdate(enabled)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, '获取自动下载设置失败')
      })
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
      setAutoUpdate(checked)
      if (!devPreview) {
        void setAutoDownloadUpdate(checked).catch(err => {
          setAutoUpdate(!checked)
          log.error({ err }, '设置自动下载失败')
        })
      }
    },
    [devPreview]
  )

  const handleInstall = useCallback(async () => {
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

  const headline = upToDate ? t('updater.window.upToDateTitle') : t('updater.window.title')

  const subtitle = upToDate
    ? t('updater.window.upToDateBody')
    : isReady && info
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
      {/* Header: app icon + title/subtitle */}
      <div className="flex gap-4 px-6 pt-5">
        <img src={appIcon} alt="" className="size-12 shrink-0 rounded-xl" draggable={false} />
        <div className="flex min-w-0 flex-col gap-0.5">
          <h1 className="text-[15px] font-bold leading-tight">{headline}</h1>
          {subtitle && <p className="text-[13px] leading-snug text-muted-foreground">{subtitle}</p>}
        </div>
      </div>

      {/* Progress bar (downloading / installing) */}
      {busy && (
        <div className="mx-6 mt-4 space-y-1.5">
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

      {/* Auto-update toggle (visible when not busy and update is available) */}
      {!busy && !upToDate && (
        <label className="mx-6 mt-4 flex cursor-pointer items-center gap-2.5">
          <Switch size="sm" checked={autoUpdate} onCheckedChange={handleAutoUpdateToggle} />
          <span className="text-[13px] text-muted-foreground">
            {t('updater.window.autoUpdate')}
          </span>
        </label>
      )}

      {/* Action buttons */}
      <div className="mt-auto flex items-center px-6 py-4">
        {isDownloading ? (
          <>
            <div className="flex-1" />
            <button
              type="button"
              className="mr-2 rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80 disabled:opacity-50"
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
              <Loader2 className="size-4 animate-spin" />
              {t('update.downloading')}
            </button>
          </>
        ) : isInstalling ? (
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
        ) : upToDate ? (
          <>
            <div className="flex-1" />
            <button
              type="button"
              className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
              onClick={closeWindow}
            >
              {t('updater.window.close')}
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80"
              onClick={handleSkip}
            >
              {t('updater.window.skipThisVersion')}
            </button>
            <div className="flex-1" />
            <button
              type="button"
              className="mr-2 rounded-md border border-border bg-secondary px-4 py-1.5 text-sm font-medium text-secondary-foreground hover:bg-secondary/80"
              onClick={closeWindow}
            >
              {t('updater.window.remindMeLater')}
            </button>
            <button
              type="button"
              className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              onClick={() => void handleInstall()}
              disabled={!devPreview && !info}
            >
              {isReady ? t('update.installNow') : t('updater.window.installUpdate')}
            </button>
          </>
        )}
      </div>
    </div>
  )
}

export default UpdaterWindow
