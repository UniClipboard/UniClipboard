import { useTranslation } from 'react-i18next'
import { useFactoryReset } from '@/pages/unlock/useFactoryReset'
import { AlertDialog } from '@/ui-kit'

/**
 * "Reset and start over": the user types RESET before the destructive confirmation is enabled; the
 * alert cannot be dismissed while the reset or the restart that follows it runs.
 */
export function UnlockKitResetAlert({
  open,
  onClose,
  onResetSucceeded,
}: {
  open: boolean
  onClose: () => void
  onResetSucceeded?: () => void
}) {
  const { t } = useTranslation()
  const reset = useFactoryReset(onClose, onResetSucceeded)
  return (
    <AlertDialog
      open={open}
      onOpenChange={next => {
        if (!next) reset.close()
      }}
      title={t('unlock.factoryReset.modal.title')}
      description={t('unlock.factoryReset.modal.warning')}
      confirmLabel={t('unlock.factoryReset.modal.confirm')}
      cancelLabel={t('unlock.factoryReset.modal.cancel')}
      destructive
      field={{
        label: t('unlock.factoryReset.modal.confirmPrompt'),
        value: reset.confirmation,
        onValueChange: reset.setConfirmation,
        placeholder: t('unlock.factoryReset.modal.confirmPlaceholder'),
      }}
      confirmDisabled={!reset.confirmed}
      busyLabel={
        reset.busy
          ? t(
              reset.restarting
                ? 'unlock.factoryReset.modal.restarting'
                : 'unlock.factoryReset.modal.resetting'
            )
          : undefined
      }
      error={reset.errorKey ? t(reset.errorKey) : undefined}
      onConfirm={() => reset.submit()}
    />
  )
}
