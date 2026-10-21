import { SettingAlert, SettingPresentationRoot } from '@ui-setting-parts'
import { useTranslation } from 'react-i18next'

type Props = {
  unavailable?: boolean
  onOpenImport: () => void | Promise<void>
  onContinue: () => void
}

/** Startup feedback leaves the original space usable and does not acknowledge Engine state. */
export function ConfigImportRejectedDialog({ unavailable, onOpenImport, onContinue }: Props) {
  const { t } = useTranslation(undefined, { keyPrefix: 'configImportRecovery' })
  return (
    <SettingPresentationRoot>
      <SettingAlert
        open
        onOpenChange={open => {
          if (!open) onContinue()
        }}
        title={t(unavailable ? 'statusReadFailed' : 'message')}
        description={t(unavailable ? 'statusReadHint' : 'preserved')}
        confirmLabel={t(unavailable ? 'retry' : 'openImport')}
        cancelLabel={t('continueOriginal')}
        onConfirm={async () => {
          await onOpenImport()
        }}
        onCancel={onContinue}
      />
    </SettingPresentationRoot>
  )
}
