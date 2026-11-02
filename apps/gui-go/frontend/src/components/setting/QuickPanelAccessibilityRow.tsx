import { SettingActionButton } from '@ui-setting-parts'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import { openUrl } from '@/host/opener'
import { createLogger } from '@/lib/logger'
import { SettingRow } from './SettingRow'
import type { AccessibilityStatus } from './useQuickPanelAccessibility'

const log = createLogger('quick-panel-accessibility-row')
const SETTINGS_URL = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'

export function QuickPanelAccessibilityRow({
  status,
  onRetry,
}: {
  status: AccessibilityStatus
  onRetry: () => void
}) {
  const { t } = useTranslation()
  const openSettings = () => {
    void openUrl(SETTINGS_URL).catch(err => {
      log.error({ err }, 'Failed to open macOS Accessibility settings')
      toast.error(t('settings.sections.quickPanel.doubleTap.openSettingsError'))
    })
  }
  return (
    <SettingRow
      label={t('settings.sections.quickPanel.accessibility.label')}
      description={t('settings.sections.quickPanel.accessibility.description')}
    >
      <div className="flex flex-wrap items-center justify-end gap-2">
        <span role="status" aria-live="polite">
          <span className="text-ui-body text-muted-foreground">
            {t(`settings.sections.quickPanel.accessibility.${status}`)}
          </span>
        </span>
        {status === 'error' ? (
          <SettingActionButton onClick={onRetry}>
            {t('settings.sections.quickPanel.accessibility.retry')}
          </SettingActionButton>
        ) : (
          status !== 'granted' && (
            <SettingActionButton onClick={openSettings} disabled={status === 'checking'}>
              {t('settings.sections.quickPanel.accessibility.openSettings')}
            </SettingActionButton>
          )
        )}
      </div>
    </SettingRow>
  )
}
