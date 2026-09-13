import { ArrowRightLeft, Plus, Smartphone } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

interface Props {
  onlineCount: number
  onAddDevice: () => void
  onSwitchSpace: () => void
  onAddMobile: () => void
}

export default function DeviceListFooter({
  onlineCount,
  onAddDevice,
  onSwitchSpace,
  onAddMobile,
}: Props) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-col gap-2 border-t border-border/50 p-3">
      <div className="flex items-center gap-2">
        <Button
          data-testid="devices-add-device"
          variant="outline"
          size="sm"
          className="min-w-0 flex-1 bg-card shadow-none"
          title={t('devices.panel.addMenu.trigger')}
          onClick={onAddDevice}
        >
          <Plus className="size-4" />
          <span className="truncate">{t('devices.panel.addMenu.trigger')}</span>
        </Button>
        <Button
          variant="outline"
          size="icon-sm"
          className="shrink-0 bg-card text-muted-foreground shadow-none"
          aria-label={t('devices.connectMobile.title')}
          title={t('devices.connectMobile.title')}
          onClick={onAddMobile}
        >
          <Smartphone className="size-3.5" />
        </Button>
      </div>
      <div className="flex min-w-0 items-center justify-between gap-2">
        <Button
          data-testid="device-switch-space"
          variant="ghost"
          size="sm"
          className="min-w-0 shrink px-2 text-muted-foreground"
          title={t('devices.switchSpace.button')}
          onClick={onSwitchSpace}
        >
          <ArrowRightLeft className="size-3.5 shrink-0" />
          <span className="truncate">{t('devices.switchSpace.button')}</span>
        </Button>
        <span className="shrink-0 text-ui-caption text-muted-foreground">
          {t('devices.thisDevice.onlineCount', { count: onlineCount })}
        </span>
      </div>
    </div>
  )
}
