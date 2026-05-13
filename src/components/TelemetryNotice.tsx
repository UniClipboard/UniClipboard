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

interface TelemetryNoticeProps {
  /** Coordinator-controlled gate. When false, the dialog stays hidden even if
   *  localStorage has no record — this lets a higher-priority startup modal
   *  display first. */
  enabled?: boolean
  /** Invoked once the user has chosen accept or opt-out. Used by
   *  `StartupModals` to advance the queue. */
  onDismiss?: () => void
}

export default function TelemetryNotice({ enabled = true, onDismiss }: TelemetryNoticeProps = {}) {
  const { t } = useTranslation()
  const { updateGeneralSetting } = useSetting()
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (enabled && !localStorage.getItem(TELEMETRY_NOTICE_KEY)) {
      setOpen(true)
    }
  }, [enabled])

  const finish = () => {
    localStorage.setItem(TELEMETRY_NOTICE_KEY, '1')
    setOpen(false)
    onDismiss?.()
  }

  const handleAccept = () => {
    finish()
  }

  const handleOptOut = async () => {
    try {
      await updateGeneralSetting({ telemetryEnabled: false, usageAnalyticsEnabled: false })
      finish()
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
