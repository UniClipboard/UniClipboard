import { memo } from 'react';
import { Alert } from 'react-native';
import { Column, Spacer } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, height as heightModifier } from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { useEngineStore } from '@/engine/store';
import { SettingsSectionItem } from '../SettingsSectionItem';
import type { AndroidSettingFieldEdit } from './SettingsFieldDialog';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const SYNC_SWITCHES: ReadonlyArray<{
  key: 'enabled' | 'on_restore' | 'files' | 'introductions';
  labelKey: string;
}> = [
  { key: 'enabled', labelKey: 'syncEnabled' },
  { key: 'on_restore', labelKey: 'syncRestore' },
  { key: 'files', labelKey: 'syncFiles' },
  { key: 'introductions', labelKey: 'syncIntroductions' },
];

/**
 * 同步进阶设置二级页(Android)。
 * 双向开关（自动写入/自动推送）已在一级设置中枢展示，此处专注设备名与进阶同步策略。
 */
export const SyncSection = memo(function SyncSection({
  onEdit,
}: {
  onEdit: (edit: AndroidSettingFieldEdit) => void;
}) {
  const { t } = useTranslation();
  const state = useEngineStore(
    useShallow((s) => ({
      snapshot: s.snapshot,
      automaticCapture: s.automaticCapture,
      updateSettings: s.updateSettings,
      setAutomaticCapture: s.setAutomaticCapture,
    }))
  );
  const settings = state.snapshot?.settings.value;

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  if (!settings) {
    return (
      <SettingsSectionItem variant="grouped">
        <SettingsListRow testID="engine-status" title={t('settings:live.loading')} />
      </SettingsSectionItem>
    );
  }

  const deviceName = settings.device_name || state.snapshot?.self.name || '—';

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="device-name"
          testID="device-name"
          title={t('settings:live.deviceName')}
          trailing={{ text: deviceName, chevron: true }}
          onPress={() =>
            onEdit({
              id: 'device-name',
              title: t('settings:live.deviceName'),
              value: settings.device_name || state.snapshot?.self.name || '',
              save: (next) =>
                state.updateSettings((old) => ({ ...old, device_name: next.trim() })),
            })
          }
        />
        {SYNC_SWITCHES.map((item) => (
          <SettingsSwitchRow
            key={item.key}
            testID={`sync-${item.key}`}
            title={t(`settings:live.${item.labelKey}`)}
            value={settings.sync[item.key]}
            onValueChange={(value) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  sync: { ...old.sync, [item.key]: value },
                }))
              )
            }
          />
        ))}
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsSwitchRow
          key="auto-capture"
          testID="auto-capture"
          title={t('settings:live.capture')}
          value={state.automaticCapture}
          onValueChange={(value) => run(state.setAutomaticCapture(value))}
        />
      </SettingsSectionItem>
    </Column>
  );
});
