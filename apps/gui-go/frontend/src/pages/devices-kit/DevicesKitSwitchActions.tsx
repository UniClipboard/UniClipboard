import { useTranslation } from 'react-i18next'
import type { useSwitchSpace } from '@/hooks/useSwitchSpace'
import { Button } from '@/ui-kit'

export function DevicesKitSwitchActions({
  model,
  onBack,
}: {
  model: ReturnType<typeof useSwitchSpace>
  onBack: () => void
}) {
  const { t } = useTranslation()
  if (model.step === 'pending')
    return (
      <Button onClick={() => void model.handleCancelPending()}>
        {t('devices.switchSpace.actions.cancel')}
      </Button>
    )
  if (model.step === 'success')
    return (
      <Button variant="primary" onClick={onBack}>
        {t('history.composite.done')}
      </Button>
    )
  if (model.step !== 'failed') return null
  if (model.errorKind === 'unreadable_history_confirmation_required')
    return (
      <Button variant="primary" onClick={() => void model.handleSubmit(true)}>
        {t('devices.switchSpace.actions.preserveAndContinue')}
      </Button>
    )
  return (
    <Button variant="primary" onClick={model.handleRetry}>
      {model.retryLabel}
    </Button>
  )
}
