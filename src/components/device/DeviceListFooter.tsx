import { ArrowRightLeft, MoreHorizontal, Plus, Settings2, Smartphone } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

interface Props {
  onlineCount: number
  onAddDevice: () => void
  onSwitchSpace: () => void
  onAddMobile: () => void
  onMobileSettings: () => void
}

export default function DeviceListFooter({
  onlineCount,
  onAddDevice,
  onSwitchSpace,
  onAddMobile,
  onMobileSettings,
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
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                variant="outline"
                size="icon-sm"
                className="shrink-0 bg-card shadow-none"
                aria-label={t('devices.panel.addMenu.otherWays')}
                title={t('devices.panel.addMenu.otherWays')}
              />
            }
          >
            <MoreHorizontal className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="top" className="w-64">
            <DropdownMenuItem onClick={onAddMobile}>
              <Smartphone className="size-4" />
              {t('devices.panel.addMenu.mobile')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={onMobileSettings}>
              <Settings2 className="size-4" />
              {t('devices.mobileSync.title')} · {t('devices.mobileSync.configure')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="flex min-w-0 items-center justify-between gap-2">
        <Button
          data-testid="device-switch-space"
          variant="ghost"
          size="sm"
          className="min-w-0 shrink px-2 text-xs text-muted-foreground"
          title={t('devices.switchSpace.button')}
          onClick={onSwitchSpace}
        >
          <ArrowRightLeft className="size-3.5 shrink-0" />
          <span className="truncate">{t('devices.switchSpace.button')}</span>
        </Button>
        <span className="shrink-0 text-xs text-muted-foreground">
          {t('devices.thisDevice.onlineCount', { count: onlineCount })}
        </span>
      </div>
    </div>
  )
}
