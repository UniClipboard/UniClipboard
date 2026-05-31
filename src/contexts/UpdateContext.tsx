import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  cancelDownload as apiCancelDownload,
  checkForUpdate,
  downloadUpdate as apiDownloadUpdate,
  getDownloadProgress,
  getInstallKind,
  installUpdate as apiInstallUpdate,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
  type DownloadEvent,
  type DownloadProgress,
  type InstallKind,
  type UpdateMetadata,
} from '@/api/updater'
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
  const { setting } = useSetting()
  const [state, setState] = useState<UpdateState>(initialState)
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false)
  const [installKind, setInstallKind] = useState<InstallKind | null>(null)
  const isSystemManaged = installKind === 'deb' || installKind === 'rpm'
  // Portable Windows zip joins deb/rpm in "can't self-install" territory: its
  // NSIS updater would install into Program Files instead of refreshing the
  // portable folder, so it's routed to the manual-download dialog too.
  const isManualUpdate = isSystemManaged || installKind === 'windowsportable'

  const activeCheckRef = useRef<Promise<UpdateMetadata | null> | null>(null)
  const activeCheckChannelRef = useRef<UpdateChannel | null>(null)
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

  // Mount-time: probe install kind so deb/rpm users get routed to the
  // package manager dialog instead of the in-app updater. One-shot — the
  // backend caches the answer.
  useEffect(() => {
    let cancelled = false
    getInstallKind()
      .then(kind => {
        if (!cancelled) setInstallKind(kind)
      })
      .catch(err => {
        if (!cancelled) log.error({ err }, '获取安装类型失败')
      })
    return () => {
      cancelled = true
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
          // Snapshot 现在带齐 currentVersion / body / date 四个字段
          // （Phase 6A 后 startup 不再补 checkForUpdate，UI 必须靠 snapshot
          // 直接渲染完整 metadata）。
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

  // Listen for scheduler-driven (or manual) `do_check_for_update` results.
  // Without this, a check that resolved AFTER mount would never reach the UI
  // —— Phase 6A removed the frontend's startup check, so `getDownloadProgress`
  // alone only catches state present at mount time. Mirrors the setState
  // shape used by `runCheckForChannel` so manual and broadcast paths converge.
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined

    void subscribeUpdateAvailable(update => {
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
    })
      .then(fn => {
        if (cancelled) {
          fn()
          return
        }
        unlisten = fn
      })
      .catch(err => {
        if (!cancelled) {
          log.error({ err }, '订阅更新可用广播失败')
        }
      })

    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [])

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
      installKind,
      isSystemManaged,
      isManualUpdate,
    }),
    [
      state,
      isCheckingUpdate,
      downloadProgress,
      checkForUpdates,
      doDownloadUpdate,
      doCancelDownload,
      doInstallUpdate,
      installKind,
      isSystemManaged,
      isManualUpdate,
    ]
  )

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
}
