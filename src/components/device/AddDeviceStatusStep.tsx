import { AlertCircle, CheckCircle2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export function AddDeviceSuccessStep() {
  const { t } = useTranslation()
  return (
    <div data-testid="add-device-success" className="flex flex-col items-center gap-3 py-8">
      <div className="flex size-14 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
        <CheckCircle2 className="size-8" />
      </div>
      <div className="text-center">
        <p className="text-ui-section font-semibold text-foreground">
          {t('devices.addDevice.success.title')}
        </p>
        <p className="mt-1 text-ui-body text-muted-foreground">
          {t('devices.addDevice.success.subtitle')}
        </p>
      </div>
    </div>
  )
}

export function AddDeviceFailureStep({ message }: { message: string }) {
  const { t } = useTranslation()
  return (
    <div className="flex flex-col items-center gap-3 py-6">
      <div className="flex size-12 items-center justify-center rounded-full bg-destructive/15 text-destructive">
        <AlertCircle className="size-7" />
      </div>
      <div className="text-center">
        <p className="text-ui-section font-semibold text-foreground">
          {t('devices.addDevice.failed.title')}
        </p>
        <p className="mt-1 text-ui-body text-muted-foreground">{message}</p>
        <p className="mt-3 text-ui-caption text-muted-foreground/70">
          {t('devices.addDevice.failed.networkHint')}
        </p>
      </div>
    </div>
  )
}
