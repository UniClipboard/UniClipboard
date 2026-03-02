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
  const isCheckingRef = useRef(false)
  const hasCheckedOnStartup = useRef(false)

  const checkForUpdates = useCallback(async () => {
    if (isCheckingRef.current) {
      return updateInfo
    }

    isCheckingRef.current = true
    setIsCheckingUpdate(true)

    try {
      const channel = setting?.general?.update_channel ?? null
      const update = await checkForUpdate(channel)
      setUpdateInfo(update)
      return update
    } finally {
      isCheckingRef.current = false
      setIsCheckingUpdate(false)
    }
  }, [updateInfo, setting?.general?.update_channel])

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

    if (!setting.general.auto_check_update) {
      return
    }

    checkForUpdates().catch(error => {
      console.error('检查更新失败:', error)
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
