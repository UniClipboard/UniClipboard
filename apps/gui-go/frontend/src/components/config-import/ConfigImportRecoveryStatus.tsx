import { SettingNotice } from '@ui-setting-parts'
import { useTranslation } from 'react-i18next'
import { useConfigImportStatus } from '@/hooks/useConfigImportStatus'
import { ConfigImportRejectedNotice } from './ConfigImportRejectedNotice'

export function ConfigImportRecoveryStatus({ onImport }: { onImport: () => void }) {
  const { t } = useTranslation(undefined, { keyPrefix: 'configImportRecovery' })
  const { data, isError, refetch } = useConfigImportStatus()
  return (
    <>
      {data?.rejectedImport && <ConfigImportRejectedNotice onImport={onImport} />}
      {isError && (
        <SettingNotice
          tone="error"
          title={t('statusReadFailed')}
          text={t('statusReadHint')}
          action={{
            label: t('retry'),
            onClick: () => {
              void refetch()
            },
          }}
        />
      )}
    </>
  )
}
