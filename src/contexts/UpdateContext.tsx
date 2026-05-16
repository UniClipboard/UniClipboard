import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  cancelDownload as apiCancelDownload,
  checkForUpdate,
  downloadUpdate as apiDownloadUpdate,
  getDownloadProgress,
  installUpdate as apiInstallUpdate,
  subscribeUpdateProgress,
  type DownloadEvent,
  type DownloadProgress,
  type UpdateMetadata,
} from '@/api/updater'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'
import { UpdateContext, type UpdateState } from './update-context'

const log = createLogger('update-context')

interface UpdateProviderProps {
  children: React.ReactNode
}

const initialState: UpdateState = {
  phase: 'idle',
  info: null,
  downloaded: 0,
  total: null,
}

export const UpdateProvider: React.FC<UpdateProviderProps> = ({ children }) => {
  const { t } = useTranslation()
  const { setting } = useSetting()
  const [state, setState] = useState<UpdateState>(initialState)
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false)

  const activeCheckRef = useRef<Promise<UpdateMetadata | null> | null>(null)
  const activeCheckChannelRef = useRef<UpdateChannel | null>(null)
  const hasCheckedOnStartup = useRef(false)
  /**
   * Versions for which a background download has already been kicked off
   * this session. Prevents the `auto-download` effect from looping when
   * a download fails and the backend returns to `Available`.
   */
  const autoDownloadAttempted = useRef<Set<string>>(new Set())
  /** Latest `state` value visible to event-driven callbacks. */
  const stateRef = useRef<UpdateState>(initialState)
  useEffect(() => {
    stateRef.current = state
  }, [state])

  const runCheckForChannel = useCallback(async (channel: UpdateChannel | null) => {
    setIsCheckingUpdate(true)
    const check = checkForUpdate(channel)
    activeCheckRef.current = check
    activeCheckChannelRef.current = channel

    try {
      const update = await check
      setState(prev => {
        if (!update) {
          return prev.phase === 'downloading' || prev.phase === 'installing'
            ? prev
            : { phase: 'idle', info: null, downloaded: 0, total: null }
        }

        if (prev.phase === 'ready' && prev.info?.version === update.version) {
          return { ...prev, info: update }
        }

        if (prev.phase === 'downloading' && prev.info?.version === update.version) {
          return { ...prev, info: update }
        }

        return {
          phase: 'available',
          info: update,
          downloaded: 0,
          total: null,
        }
      })
      return update
    } finally {
      if (activeCheckRef.current === check) {
        activeCheckRef.current = null
        activeCheckChannelRef.current = null
        setIsCheckingUpdate(false)
      }
    }
  }, [])

  const checkForUpdates = useCallback(
    async (channelOverride?: UpdateChannel | null) => {
      const channel =
        channelOverride === undefined ? (setting?.general?.updateChannel ?? null) : channelOverride
      const activeCheck = activeCheckRef.current

      if (activeCheck) {
        if (activeCheckChannelRef.current === channel) {
          return activeCheck
        }

        await activeCheck.catch(error => {
          log.error({ err: error }, '等待上一次更新检查完成失败')
        })
      }

      return runCheckForChannel(channel)
    },
    [runCheckForChannel, setting?.general?.updateChannel]
  )

  const doDownloadUpdate = useCallback(async () => {
    if (stateRef.current.phase !== 'available') {
      return
    }
    const info = stateRef.current.info
    setState(prev => ({
      ...prev,
      phase: 'downloading',
      downloaded: 0,
      total: null,
    }))
    try {
      await apiDownloadUpdate()
      // Backend transitions to Ready; the broadcast `Finished` event also
      // fires below, but it doesn't carry the final downloaded total, so
      // we authoritatively set `ready` here.
      setState(prev => ({
        ...prev,
        phase: 'ready',
        info: info ?? prev.info,
      }))
    } catch (error) {
      // Backend returns to Available on failure/cancel; reflect that
      // locally so the Sidebar icon falls back to amber.
      setState(prev => ({
        ...prev,
        phase: 'available',
        downloaded: 0,
        total: null,
      }))
      throw error
    }
  }, [])

  const doCancelDownload = useCallback(async () => {
    try {
      await apiCancelDownload()
    } catch (error) {
      log.error({ err: error }, '取消下载请求失败')
      throw error
    }
  }, [])

  const doInstallUpdate = useCallback(async () => {
    const phaseBefore = stateRef.current.phase
    setState(prev => ({ ...prev, phase: 'installing' }))
    try {
      await apiInstallUpdate(progress => {
        setState(prev => ({
          ...prev,
          phase: progress.phase === 'installing' ? 'installing' : 'downloading',
          downloaded: progress.downloaded,
          total: progress.total,
        }))
      })
    } catch (error) {
      // Restore the prior phase so the user can retry without losing
      // cached bytes (backend kept `Ready` on install failure).
      setState(prev => ({
        ...prev,
        phase: phaseBefore === 'idle' ? 'idle' : phaseBefore,
      }))
      throw error
    }
  }, [])

  const handleDownloadEvent = useCallback((event: DownloadEvent) => {
    switch (event.event) {
      case 'Started':
        setState(prev => ({
          ...prev,
          phase: prev.phase === 'installing' ? 'installing' : 'downloading',
          downloaded: 0,
          total: event.data.contentLength,
        }))
        break
      case 'Progress':
        setState(prev => ({
          ...prev,
          phase: prev.phase === 'installing' ? 'installing' : 'downloading',
          downloaded: prev.downloaded + event.data.chunkLength,
        }))
        break
      case 'Finished':
        setState(prev => ({
          ...prev,
          phase: prev.phase === 'installing' ? 'installing' : 'ready',
          total: prev.total ?? prev.downloaded,
        }))
        break
      case 'Failed':
        setState(prev => ({
          ...prev,
          phase: prev.info ? 'available' : 'idle',
          downloaded: 0,
          total: null,
        }))
        break
    }
  }, [])

  // Mount-time: sync backend snapshot, then attach broadcast listener.
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined

    void getDownloadProgress()
      .then(snapshot => {
        if (cancelled) return
        if (snapshot.phase === 'idle') return
        setState({
          phase: snapshot.phase,
          // 启动期 sync 路径只拿得到 version 字符串，无法回填 release notes
          // (`body`) 与发布日期 (`date`) —— 用 null 占位，等下一次主动
          // checkForUpdate 再覆盖完整 metadata。
          info: snapshot.version
            ? {
                version: snapshot.version,
                currentVersion: snapshot.version,
                body: null,
                date: null,
              }
            : null,
          downloaded: snapshot.downloaded,
          total: snapshot.total,
        })
      })
      .catch(err => {
        if (!cancelled) {
          log.error({ err }, '同步下载状态失败')
        }
      })

    void subscribeUpdateProgress(handleDownloadEvent)
      .then(fn => {
        if (cancelled) {
          fn()
          return
        }
        unlisten = fn
      })
      .catch(err => {
        if (!cancelled) {
          log.error({ err }, '订阅更新进度事件失败')
        }
      })

    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [handleDownloadEvent])

  // Startup auto-check (gated by `autoCheckUpdate`).
  useEffect(() => {
    if (!setting?.general || hasCheckedOnStartup.current) {
      return
    }

    hasCheckedOnStartup.current = true

    if (!setting.general.autoCheckUpdate) {
      return
    }

    checkForUpdates().catch(error => {
      log.error({ err: error }, '检查更新失败')
      toast.error(t('update.checkFailed'))
    })
  }, [setting?.general, checkForUpdates, t])

  // Background auto-download: whenever state is `available` and the
  // setting is on, kick off a silent download. Tracks attempted versions
  // to avoid retry loops if download fails.
  useEffect(() => {
    if (!setting?.general?.autoDownloadUpdate) return
    if (!setting?.general?.autoCheckUpdate) return
    if (state.phase !== 'available') return
    if (!state.info) return
    if (autoDownloadAttempted.current.has(state.info.version)) return

    autoDownloadAttempted.current.add(state.info.version)
    void doDownloadUpdate().catch(err => {
      log.error({ err }, '自动后台下载失败')
    })
  }, [
    setting?.general?.autoDownloadUpdate,
    setting?.general?.autoCheckUpdate,
    state.phase,
    state.info,
    doDownloadUpdate,
  ])

  const downloadProgress = useMemo<DownloadProgress>(
    () => ({
      downloaded: state.downloaded,
      total: state.total,
      phase: state.phase,
    }),
    [state.downloaded, state.total, state.phase]
  )

  const value = useMemo(
    () => ({
      state,
      isCheckingUpdate,
      updateInfo: state.info,
      downloadProgress,
      checkForUpdates,
      downloadUpdate: doDownloadUpdate,
      cancelDownload: doCancelDownload,
      installUpdate: doInstallUpdate,
    }),
    [
      state,
      isCheckingUpdate,
      downloadProgress,
      checkForUpdates,
      doDownloadUpdate,
      doCancelDownload,
      doInstallUpdate,
    ]
  )

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
}
