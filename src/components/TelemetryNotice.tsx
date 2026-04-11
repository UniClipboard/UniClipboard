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
import { createLogger } from '@/lib/logger'

const log = createLogger('telemetry-notice')

const TELEMETRY_NOTICE_KEY = 'uc-telemetry-notice-seen'

export default function TelemetryNotice() {
  const { t } = useTranslation()
  const { updateGeneralSetting } = useSetting()
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!localStorage.getItem(TELEMETRY_NOTICE_KEY)) {
      setOpen(true)
    }
  }, [])

  const markSeen = () => {
    localStorage.setItem(TELEMETRY_NOTICE_KEY, '1')
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
      log.error({ err: error }, 'Failed to disable telemetry')
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
