import { SettingAlert, SettingPresentationRoot } from '@ui-setting-parts'
import { useTranslation } from 'react-i18next'

type Props = { open: boolean; busy: boolean; onConfirm: () => Promise<void>; onCancel: () => void }

/** Consent here is separate from the ordinary device-identity move confirmation. */
export function ConfigImportReplacementDialog({ open, busy, onConfirm, onCancel }: Props) {
  const { t } = useTranslation(undefined, { keyPrefix: 'configImportRecovery' })
  return (
    <SettingPresentationRoot>
      <SettingAlert
        open={open}
        onOpenChange={value => {
          if (!value && !busy) onCancel()
        }}
        title={t('replaceTitle')}
        description={t('replaceWarning')}
        destructive
        confirmLabel={t('replaceConfirm')}
        cancelLabel={t('keepStaging')}
        busyLabel={busy ? t('replacing') : undefined}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    </SettingPresentationRoot>
  )
}
