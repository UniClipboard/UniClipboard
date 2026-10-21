import { useTranslation } from 'react-i18next'
import { useSetting } from '@/hooks/useSetting'
import type { TrayIcon } from '@/types/setting'
import { SettingSelect } from '../SettingControls'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

const TRAY_ICONS: TrayIcon[] = ['glyph', 'cat']

export function TrayIconSettings() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()

  // The desktop host follows the saved value through the daemon's settings events and redraws the tray itself.
  const [trayIcon, setTrayIcon] = useOptimisticSetting<TrayIcon>(
    setting?.general.trayIcon ?? 'glyph',
    next => updateGeneralSetting({ trayIcon: next }),
    { failureLog: 'Failed to change tray-icon setting' }
  )

  return (
    <SettingGroup title={t('settings.sections.general.trayTitle')}>
      <SettingRow
        label={t('settings.sections.general.trayIcon.label')}
        description={t('settings.sections.general.trayIcon.description')}
      >
        <SettingSelect
          label={t('settings.sections.general.trayIcon.label')}
          value={trayIcon}
          onValueChange={next => setTrayIcon(next as TrayIcon)}
          items={TRAY_ICONS.map(icon => ({
            value: icon,
            label: t(`settings.sections.general.trayIcon.options.${icon}`),
          }))}
        />
      </SettingRow>
    </SettingGroup>
  )
}
