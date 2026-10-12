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
import { useTelemetryNotice, type TelemetryNoticeProps } from '@/hooks/useTelemetryNotice'

export default function TelemetryNotice({ enabled = true, onDismiss }: TelemetryNoticeProps = {}) {
  const { t } = useTranslation()
  const { open, accept, optOut } = useTelemetryNotice({ enabled, onDismiss })

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
          <AlertDialogCancel onClick={optOut}>
            {t('settings.sections.general.telemetry.notice.optOut')}
          </AlertDialogCancel>
          <AlertDialogAction onClick={accept}>
            {t('settings.sections.general.telemetry.notice.accept')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
