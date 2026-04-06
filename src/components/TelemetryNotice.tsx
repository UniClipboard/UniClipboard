import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'

const TELEMETRY_NOTICE_KEY_PREFIX = 'uc-telemetry-notice-seen-v'

function getNoticeStorageKey(version: string): string {
  // Key by major.minor so the notice re-appears on feature releases, not patches.
  const [major, minor] = version.split('.')
  return `${TELEMETRY_NOTICE_KEY_PREFIX}${major}.${minor}`
}

export default function TelemetryNotice() {
  const { t } = useTranslation()
  const { updateGeneralSetting } = useSetting()
  const [open, setOpen] = useState(false)
  const [storageKey, setStorageKey] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    getVersion()
      .then(version => {
        if (cancelled) return
        const key = getNoticeStorageKey(version)
        if (!localStorage.getItem(key)) {
          setStorageKey(key)
          setOpen(true)
        }
      })
      .catch(console.error)

    return () => {
      cancelled = true
    }
  }, [])

  const markSeen = () => {
    if (storageKey) {
      localStorage.setItem(storageKey, '1')
    }
  }

  const handleAccept = () => {
    markSeen()
    setOpen(false)
  }

  const handleOptOut = async () => {
    try {
      await updateGeneralSetting({ telemetryEnabled: false })
      markSeen()
      setOpen(false)
    } catch (error) {
      console.error('Failed to disable telemetry:', error)
      // Don't close — let the user retry or accept instead.
    }
  }

  return (
    <AlertDialog open={open}>
      <AlertDialogContent className="bg-card text-card-foreground">
        <AlertDialogHeader>
          <AlertDialogTitle>
            {t('settings.sections.general.telemetry.notice.title')}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t('settings.sections.general.telemetry.notice.body')}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={handleOptOut}>
            {t('settings.sections.general.telemetry.notice.optOut')}
          </AlertDialogCancel>
          <AlertDialogAction onClick={handleAccept}>
            {t('settings.sections.general.telemetry.notice.accept')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
