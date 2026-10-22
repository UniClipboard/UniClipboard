import { useTranslation } from 'react-i18next'
import type { useSwitchSpace } from '@/hooks/useSwitchSpace'
import { TaskPanel } from '@/ui-kit'
import { DevicesKitJoinInput } from './DevicesKitJoinInput'
import { DevicesKitSwitchActions } from './DevicesKitSwitchActions'
import { DevicesKitSwitchStatus } from './DevicesKitSwitchStatus'

export function DevicesKitSwitchSpace({
  model,
  onBack,
}: {
  model: ReturnType<typeof useSwitchSpace>
  onBack: () => void
}) {
  const { t } = useTranslation()
  const busy = model.step === 'migrating' || model.step === 'processing'
  return (
    <div data-testid="switch-space-panel">
      {model.step === 'input' ? (
        <DevicesKitJoinInput model={model} onBack={onBack} />
      ) : (
        <TaskPanel
          title={t(`devices.switchSpace.${model.step}.title`)}
          back={
            model.step === 'success'
              ? undefined
              : { label: t('devices.switchSpace.backToDevices'), onClick: onBack, disabled: busy }
          }
          actions={<DevicesKitSwitchActions model={model} onBack={onBack} />}
        >
          <DevicesKitSwitchStatus model={model} />
        </TaskPanel>
      )}
    </div>
  )
}
