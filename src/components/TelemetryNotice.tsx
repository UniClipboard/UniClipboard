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

  useEffect(() => {
    let cancelled = false

    getVersion()
      .then(version => {
        if (cancelled) return
        const key = getNoticeStorageKey(version)
        if (!localStorage.getItem(key)) {
          setOpen(true)
          localStorage.setItem(key, '1')
        }
      })
      .catch(console.error)

    return () => {
      cancelled = true
    }
  }, [])

  const handleOptOut = async () => {
    try {
      await updateGeneralSetting({ telemetryEnabled: false })
    } catch (error) {
      console.error('Failed to disable telemetry:', error)
    }
    setOpen(false)
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
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
          <AlertDialogAction onClick={() => setOpen(false)}>
            {t('settings.sections.general.telemetry.notice.accept')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
