import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UpdateContext } from './update-context'
import {
  checkForUpdate,
  installUpdate as apiInstallUpdate,
  type UpdateMetadata,
  type DownloadProgress,
} from '@/api/updater'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'

const log = createLogger('update-context')

interface UpdateProviderProps {
  children: React.ReactNode
}

export const UpdateProvider: React.FC<UpdateProviderProps> = ({ children }) => {
  const { t } = useTranslation()
  const { setting } = useSetting()
  const [updateInfo, setUpdateInfo] = useState<UpdateMetadata | null>(null)
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false)
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress>({
    downloaded: 0,
    total: null,
    phase: 'idle',
  })
  const activeCheckRef = useRef<Promise<UpdateMetadata | null> | null>(null)
  const activeCheckChannelRef = useRef<UpdateChannel | null>(null)
  const hasCheckedOnStartup = useRef(false)

  const runCheckForChannel = useCallback(async (channel: UpdateChannel | null) => {
    setIsCheckingUpdate(true)
    const check = checkForUpdate(channel)
    activeCheckRef.current = check
    activeCheckChannelRef.current = channel

    try {
      const update = await check
      setUpdateInfo(update)
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

  const doInstallUpdate = useCallback(async () => {
    setDownloadProgress({ downloaded: 0, total: null, phase: 'downloading' })
    try {
      await apiInstallUpdate(progress => {
        setDownloadProgress(progress)
      })
    } catch (error) {
      setDownloadProgress({ downloaded: 0, total: null, phase: 'idle' })
      throw error
    }
  }, [])

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

  const value = useMemo(
    () => ({
      updateInfo,
      isCheckingUpdate,
      downloadProgress,
      checkForUpdates,
      installUpdate: doInstallUpdate,
    }),
    [updateInfo, isCheckingUpdate, downloadProgress, checkForUpdates, doInstallUpdate]
  )

  return <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
}
