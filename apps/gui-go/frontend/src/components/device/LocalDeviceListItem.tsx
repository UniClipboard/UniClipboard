import { memo } from 'react'
import { useTranslation } from 'react-i18next'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import DeviceListItem from '@/components/device/DeviceListItem'
import { useSettingSelector } from '@/hooks/useSetting'

function LocalDeviceListItem({
  name,
  status,
  selected,
  onSelect,
}: {
  name: string
  status?: DeviceRowStatus
  selected: boolean
  onSelect: () => void
}) {
  const { t } = useTranslation()
  const loaded = useSettingSelector(context => context.setting !== null)
  const syncActive = useSettingSelector(({ setting }) => setting?.sync.syncEnabled !== false)
  const current: DeviceRowStatus = status ?? {
    kind: !loaded ? 'unknown' : syncActive ? 'online' : 'paused',
    label: !loaded
      ? t('common.loading')
      : t(`devices.thisDevice.${syncActive ? 'syncActive' : 'syncPaused'}`),
  }
  return (
    <DeviceListItem
      testId="device-local"
      name={name}
      tone={
        current.kind === 'online'
          ? 'success'
          : ['paused', 'unknown'].includes(current.kind)
            ? 'off'
            : 'warning'
      }
      status={current}
      selected={selected}
      onSelect={onSelect}
    />
  )
}

export default memo(LocalDeviceListItem)
