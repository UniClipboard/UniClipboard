import { Send } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { DeviceTrustRelationship } from '@/api/daemon/device-trust'
import CopyIconButton from '@/components/device/CopyIconButton'
import { getDeviceIcon } from '@/components/device/device-utils'
import PanelFactRow from '@/components/device/PanelFactRow'
import StatusDot from '@/components/device/StatusDot'

export default function RemovedDevicePanel({ device }: { device: DeviceTrustRelationship }) {
  const { t } = useTranslation()
  const Icon = getDeviceIcon(device.displayName)

  return (
    <div data-testid="removed-device-detail" className="@container min-h-full w-full bg-muted/20">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-7 px-5 py-8 @md:px-8 @lg:py-10">
        <header className="flex items-center gap-4">
          <div className="flex size-16 shrink-0 items-center justify-center rounded-xl border border-border/60 bg-card text-muted-foreground">
            <Icon className="size-8" strokeWidth={1.5} />
          </div>
          <div className="min-w-0 flex-1">
            <h3
              title={device.displayName}
              className="truncate text-ui-title font-semibold text-foreground"
            >
              {device.displayName || t('devices.list.labels.unknownDevice')}
            </h3>
            <p className="mt-2 flex items-center gap-2 text-ui-caption">
              <StatusDot tone="info" />
              <span className="font-medium text-info">
                {t('devices.memberRemoval.notificationPending.title')}
              </span>
            </p>
          </div>
        </header>

        <section className="overflow-hidden rounded-xl border border-border/60 bg-card text-card-foreground">
          <div className="flex items-start gap-3 px-5 py-4 @md:px-6">
            <Send className="mt-0.5 size-4 shrink-0 text-info" aria-hidden="true" />
            <p data-testid="removed-device-notice" className="text-ui-body text-muted-foreground">
              {t('devices.memberRemoval.notificationPending.description')}
            </p>
          </div>
          <div className="border-t border-border/50 px-5 py-2 @md:px-6">
            <PanelFactRow label={t('devices.panel.fields.peerId')}>
              <span className="inline-flex min-w-0 max-w-full items-center gap-1">
                <span
                  className="min-w-0 truncate font-mono text-ui-caption font-medium"
                  title={device.deviceId}
                >
                  {device.deviceId}
                </span>
                <CopyIconButton value={device.deviceId} />
              </span>
            </PanelFactRow>
          </div>
        </section>
      </div>
    </div>
  )
}
