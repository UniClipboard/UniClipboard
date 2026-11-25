import { Alert } from 'react-native';
import { Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { SettingsNavRow, SettingsToggle } from './common';
import type { IosSettingFieldEdit } from './types';

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
 * 同步进阶设置（二级页遵循 iOS 设置惯例：无前导色块、左对齐排版；
 * 自动写入/自动推送已在一级页内联展示，此处不重复，专注进阶选项）。
 */
export function SyncPage({ onEdit }: { onEdit: (edit: IosSettingFieldEdit) => void }) {
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

  return (
    <IosSheetPage title={t('settings:category.sync')}>
      <IosSheetForm>
        {settings ? (
          <>
            <Section>
              <SettingsNavRow
                testID="device-name"
                title={t('settings:live.deviceName')}
                value={settings.device_name || state.snapshot?.self.name || '—'}
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
                <SettingsToggle
                  key={item.key}
                  testID={`sync-${item.key}`}
                  label={t(`settings:live.${item.labelKey}`)}
                  isOn={settings.sync[item.key]}
                  onIsOnChange={(value) =>
                    run(
                      state.updateSettings((old) => ({
                        ...old,
                        sync: { ...old.sync, [item.key]: value },
                      }))
                    )
                  }
                />
              ))}
            </Section>

            <Section footer={<SwiftUIText>{t('settings:live.captureHint')}</SwiftUIText>}>
              <SettingsToggle
                testID="auto-capture"
                label={t('settings:live.capture')}
                isOn={state.automaticCapture}
                onIsOnChange={(v) => run(state.setAutomaticCapture(v))}
              />
            </Section>
          </>
        ) : (
          <Section>
            <SwiftUIText>{t('settings:live.loading')}</SwiftUIText>
          </Section>
        )}
      </IosSheetForm>
    </IosSheetPage>
  );
}
