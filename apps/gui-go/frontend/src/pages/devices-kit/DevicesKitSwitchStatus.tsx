import { useTranslation } from 'react-i18next'
import type { useSwitchSpace } from '@/hooks/useSwitchSpace'
import { InlineNotice, ProgressBar, Text } from '@/ui-kit'

export function DevicesKitSwitchStatus({ model }: { model: ReturnType<typeof useSwitchSpace> }) {
  const { t } = useTranslation()
  const key = 'devices.switchSpace.'
  if (model.step === 'failed')
    return <InlineNotice tone="error" title={model.failureMessage ?? t(`${key}failed.title`)} />
  if (model.step === 'success' && model.result)
    return (
      <div data-testid="switch-space-success" role="status" className="flex flex-col gap-4">
        <InlineNotice
          tone="success"
          title={t(`${key}success.subtitle`, {
            count: model.result.joinedSpace.migratedRecords ?? 0,
          })}
        />
        {(model.result.joinedSpace.preservedUnreadableRecords ?? 0) > 0 && (
          <Text tone="secondary">
            {t(`${key}success.preservedUnreadable`, {
              count: model.result.joinedSpace.preservedUnreadableRecords,
            })}
          </Text>
        )}
      </div>
    )
  return (
    <div role="status" className="flex flex-col gap-4">
      <ProgressBar label={t(`${key}${model.step}.title`)} />
      <Text tone="secondary">
        {t(
          `${key}${model.step === 'migrating' ? 'migrating.phase.preparing' : `${model.step}.subtitle`}`
        )}
      </Text>
    </div>
  )
}
