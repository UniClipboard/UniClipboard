import { useEffect } from 'react';
import { Alert } from 'react-native';
import { Section } from '@expo/ui/swift-ui';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { SettingsNavRow, statusGreen, statusOrange } from './common';
import type { IosSettingFieldEdit } from './types';

export function NetworkPage({ onEdit }: { onEdit: (edit: IosSettingFieldEdit) => void }) {
  const { t } = useTranslation();
  const state = useEngineStore(
    useShallow((s) => ({
      snapshot: s.snapshot,
      devices: s.devices,
      updateSettings: s.updateSettings,
    }))
  );

  useEffect(() => {
    void useEngineStore.getState().refreshSnapshot();
  }, []);

  const settings = state.snapshot?.settings.value;
  const bindings = state.devices
    .flatMap((device) => device.bindings ?? [])
    .filter((binding) => binding.instance_id === 'builtin.tailcat');
  const connected = bindings.filter((binding) => binding.connection === 'connected').length;
  const runtime = state.snapshot?.runtime;

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  return (
    <IosSheetPage title={t('settings:category.network')}>
      <IosSheetForm>
        <Section>
          <SettingsNavRow
            testID="network-runtime"
            title={t('settings:live.runtime')}
            value={runtime ? t(`settings:live.status.${runtime}`) : '—'}
            valueColor={runtime === 'active' ? statusGreen : statusOrange}
            readOnly
          />
          <SettingsNavRow
            testID="network-tailcat"
            title={t('settings:live.tailcat')}
            value={t('settings:live.connected', { connected, total: bindings.length })}
            readOnly
          />
        </Section>

        {settings ? (
          <Section>
            <SettingsNavRow
              testID="rendezvous-url"
              title={t('settings:live.rendezvous')}
              value={settings.rendezvous_url || t('settings:live.default')}
              onPress={() =>
                onEdit({
                  id: 'rendezvous-url',
                  title: t('settings:live.rendezvous'),
                  value: settings.rendezvous_url,
                  placeholder: t('settings:live.default'),
                  save: (next) =>
                    state.updateSettings((old) => ({ ...old, rendezvous_url: next.trim() })),
                })
              }
            />
            <SettingsNavRow
              testID="rendezvous-reset"
              title={t('settings:live.resetRendezvous')}
              showsChevron={false}
              onPress={() =>
                run(state.updateSettings((old) => ({ ...old, rendezvous_url: '' })))
              }
            />
          </Section>
        ) : null}
      </IosSheetForm>
    </IosSheetPage>
  );
}
