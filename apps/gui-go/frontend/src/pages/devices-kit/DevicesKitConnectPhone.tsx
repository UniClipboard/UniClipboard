import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  MobileSyncSettingsView,
  RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { ChoiceList, TaskPanel } from '@/ui-kit'
import { DevicesKitPhoneSetup } from './DevicesKitPhoneSetup'

export function DevicesKitConnectPhone({
  onBack,
  onDirect,
  onSuccess,
  onBusyChange,
  onSettingsChange,
  onConfigure,
}: {
  onBack: () => void
  onDirect: () => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
  onBusyChange: (busy: boolean) => void
  onSettingsChange: (settings: MobileSyncSettingsView) => void
  onConfigure: () => void
}) {
  const { t } = useTranslation()
  const [method, setMethod] = useState<'choose' | 'regular'>('choose')
  if (method === 'regular')
    return (
      <DevicesKitPhoneSetup
        onBack={() => setMethod('choose')}
        onSuccess={onSuccess}
        onBusyChange={onBusyChange}
        onSettingsChange={onSettingsChange}
        onConfigure={onConfigure}
      />
    )
  return (
    <TaskPanel
      title={t('devices.tasks.phoneTitle')}
      description={t('devices.tasks.phoneDescription')}
      stage={t('devices.tasks.step', { current: 1, total: 2 })}
      back={{ label: t('devices.switchSpace.backToDevices'), onClick: onBack }}
    >
      <ChoiceList
        label={t('devices.connectMobile.method')}
        onSelect={id => {
          if (id === 'regular') setMethod('regular')
          else onDirect()
        }}
        items={[
          {
            id: 'regular',
            icon: 'phone',
            title: t('devices.connectMobile.regular'),
            description: t('devices.connectMobile.regularDescription'),
          },
          {
            id: 'direct',
            icon: 'add',
            title: t('devices.tasks.directTitle'),
            description: t('devices.connectMobile.directDescription'),
          },
        ]}
      />
    </TaskPanel>
  )
}
