import { SettingNotice } from '@ui-setting-parts'
import { useTranslation } from 'react-i18next'

type Props = { onImport: () => void }

/** Persistent settings feedback rendered only for an authoritative rejected import. */
export function ConfigImportRejectedNotice({ onImport }: Props) {
  const { t } = useTranslation(undefined, { keyPrefix: 'configImportRecovery' })
  return (
    <SettingNotice
      tone="warning"
      title={t('message')}
      text={t('preserved')}
      action={{ label: t('reimport'), onClick: onImport }}
    />
  )
}
