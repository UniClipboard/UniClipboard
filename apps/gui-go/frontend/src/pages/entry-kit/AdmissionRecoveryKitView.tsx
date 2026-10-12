import { useTranslation } from 'react-i18next'
import type { ProfileRecoveryResponse } from '@/api/generated/types.gen'
import { useDiagnosticsExport } from '@/hooks/useDiagnosticsExport'
import { Button, ButtonRow, EntryPage, Text } from '@/ui-kit'
import appIcon from '@/updater/app-icon.png'
import { EntryKitFacts } from './EntryKitFacts'
import { EntryKitFrame } from './EntryKitFrame'

/**
 * Saved local data that could not be opened safely (design R3): the daemon's classification and
 * recommendation, and diagnostics export. Data stays untouched; no repair is offered here.
 */
export function AdmissionRecoveryKitView({ status }: { status: ProfileRecoveryResponse }) {
  const { t } = useTranslation()
  const { exportState, exportDiagnostics } = useDiagnosticsExport()
  const admission = status.admission
  return (
    <EntryKitFrame>
      <EntryPage
        appIcon={appIcon}
        status={{ text: t('profileAdmissionRecovery.category'), tone: 'critical', icon: 'info' }}
        title={t('profileAdmissionRecovery.title')}
        description={t('profileAdmissionRecovery.description')}
      >
        {admission && (
          <EntryKitFacts
            label={t('profileAdmissionRecovery.category')}
            facts={[
              {
                id: 'issue',
                label: t('profileAdmissionRecovery.issue'),
                value: t(`profileAdmissionRecovery.categories.${admission.category}`),
              },
              {
                id: 'stage',
                label: t('profileAdmissionRecovery.stage'),
                value: t(`profileAdmissionRecovery.stages.${admission.stage}`),
              },
              {
                id: 'action',
                label: t('profileAdmissionRecovery.nextStep'),
                value: t(`profileAdmissionRecovery.actions.${admission.action}`),
              },
            ]}
          />
        )}
        <div className="flex flex-col gap-1">
          <Text emphasized>{t('profileAdmissionRecovery.preserved')}</Text>
          <Text role="caption" tone="secondary">
            {t('profileAdmissionRecovery.help')}
          </Text>
        </div>
        <ButtonRow>
          <Button
            icon="download"
            busy={exportState === 'exporting'}
            onClick={() => void exportDiagnostics()}
          >
            {t(
              exportState === 'exporting'
                ? 'profileAdmissionRecovery.exporting'
                : 'profileAdmissionRecovery.export'
            )}
          </Button>
        </ButtonRow>
        {exportState === 'done' && (
          <p role="status" className="m-0">
            <Text tone="positive">{t('profileAdmissionRecovery.exported')}</Text>
          </p>
        )}
        {exportState === 'failed' && (
          <p role="alert" className="m-0">
            <Text tone="critical">{t('profileAdmissionRecovery.exportFailed')}</Text>
          </p>
        )}
      </EntryPage>
    </EntryKitFrame>
  )
}
