import { useEffect } from 'react';
import { Alert } from 'react-native';
import { Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { cloudUserLabel } from '@/lib/cloudAuth';
import { useCloudStore } from '@/stores/cloudStore';
import { SettingsNavRow, SettingsToggle, statusGreen, statusOrange } from './common';
import type { IosSettingFieldEdit } from './types';

const DEFAULT_RELAY_MAX_MB = 25;

export function NetworkPage({ onEdit }: { onEdit: (edit: IosSettingFieldEdit) => void }) {
  const { t } = useTranslation();
  const cloud = useCloudStore(
    useShallow((s) => ({
      session: s.session,
      busy: s.busy,
      hydrate: s.hydrate,
      signIn: s.signIn,
      provisionRelay: s.provisionRelay,
    }))
  );
  const state = useEngineStore(
    useShallow((s) => ({
      snapshot: s.snapshot,
      devices: s.devices,
      updateSettings: s.updateSettings,
    }))
  );

  useEffect(() => {
    void useEngineStore.getState().refreshSnapshot();
    void cloud.hydrate();
  }, [cloud.hydrate]);

  const settings = state.snapshot?.settings.value;
  const bindings = state.devices
    .flatMap((device) => device.bindings ?? [])
    .filter((binding) => binding.instance_id === 'builtin.tailcat');
  const connected = bindings.filter((binding) => binding.connection === 'connected').length;
  const relayReady = bindings.filter((binding) => Boolean(binding.relay_route_ready)).length;
  const runtime = state.snapshot?.runtime;
  const relay = settings?.sync.relay ?? { enabled: false };
  const relayMaxMB = relay.max_bytes && relay.max_bytes > 0
    ? Math.max(1, Math.round(relay.max_bytes / (1024 * 1024)))
    : DEFAULT_RELAY_MAX_MB;

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  const handleCloudAction = () => {
    if (cloud.busy) return;
    const action = cloud.session ? cloud.provisionRelay() : cloud.signIn();
    void action
      .then(() =>
        Alert.alert(
          t('settings:live.cloudSignInSuccessTitle'),
          t('settings:live.cloudSignInSuccessBody')
        )
      )
      .catch((error) =>
        Alert.alert(
          t('settings:live.cloudSignInFailed'),
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
          <SettingsNavRow
            testID="network-relay-ready"
            title={t('settings:live.relayReadyTitle')}
            value={t('settings:live.relayReadyCount', { ready: relayReady, total: bindings.length })}
            valueColor={relay.enabled && relayReady > 0 ? statusGreen : undefined}
            readOnly
          />
        </Section>

        {settings ? (
          <>
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

            <Section
              header={<SwiftUIText>{t('settings:live.relaySection')}</SwiftUIText>}
              footer={<SwiftUIText>{t('settings:live.relayFooter')}</SwiftUIText>}
            >
              <SettingsToggle
                testID="relay-enabled"
                label={t('settings:live.relayEnabled')}
                isOn={Boolean(relay.enabled)}
                onIsOnChange={(enabled) =>
                  run(
                    state.updateSettings((old) => ({
                      ...old,
                      sync: {
                        ...old.sync,
                        relay: { ...(old.sync.relay ?? { enabled: false }), enabled },
                      },
                    }))
                  )
                }
              />
              <SettingsNavRow
                testID="relay-url"
                title={t('settings:live.relayUrl')}
                value={relay.url || t('settings:live.relayDefault')}
                onPress={() =>
                  onEdit({
                    id: 'relay-url',
                    title: t('settings:live.relayUrl'),
                    value: relay.url ?? '',
                    placeholder: 'https://relay.uniclipboard.app',
                    save: (next) =>
                      state.updateSettings((old) => ({
                        ...old,
                        sync: {
                          ...old.sync,
                          relay: {
                            ...(old.sync.relay ?? { enabled: false }),
                            url: next.trim(),
                          },
                        },
                      })),
                  })
                }
              />
              {relay.url ? (
                <SettingsNavRow
                  testID="relay-url-reset"
                  title={t('settings:live.relayResetUrl')}
                  showsChevron={false}
                  onPress={() =>
                    run(
                      state.updateSettings((old) => ({
                        ...old,
                        sync: {
                          ...old.sync,
                          relay: { ...(old.sync.relay ?? { enabled: false }), url: '' },
                        },
                      }))
                    )
                  }
                />
              ) : null}
              <SettingsNavRow
                testID="relay-max-bytes"
                title={t('settings:live.relayMaxSize')}
                value={`${relayMaxMB} MB`}
                onPress={() =>
                  onEdit({
                    id: 'relay-max-bytes',
                    title: t('settings:live.relayMaxSize'),
                    value: String(relayMaxMB),
                    placeholder: String(DEFAULT_RELAY_MAX_MB),
                    save: async (next) => {
                      const mb = Number.parseInt(next.trim(), 10);
                      if (!Number.isFinite(mb) || mb < 1 || mb > 4096) {
                        throw new Error(t('settings:live.relayMaxSizeInvalid'));
                      }
                      await state.updateSettings((old) => ({
                        ...old,
                        sync: {
                          ...old.sync,
                          relay: {
                            ...(old.sync.relay ?? { enabled: false }),
                            max_bytes: mb * 1024 * 1024,
                          },
                        },
                      }));
                    },
                  })
                }
              />
              {!relay.url ? (
                <SettingsNavRow
                  testID="relay-cloud-login"
                  title={
                    cloud.busy
                      ? t('settings:live.cloudSigningIn')
                      : cloud.session
                        ? relay.has_access_key
                          ? t('settings:live.relayRefreshFromCloud')
                          : t('settings:live.relayEnableFromCloud')
                        : t('settings:live.cloudSignIn')
                  }
                  value={cloud.session ? cloudUserLabel(cloud.session.user) : undefined}
                  showsChevron={!cloud.busy}
                  disabled={cloud.busy}
                  onPress={handleCloudAction}
                />
              ) : null}
              <SettingsNavRow
                testID="relay-access-key"
                title={t('settings:live.relayAccessKey')}
                value={relay.has_access_key ? t('settings:live.relayKeyConfigured') : t('settings:live.relayKeyNotSet')}
                valueColor={relay.has_access_key ? statusGreen : undefined}
                onPress={() =>
                  onEdit({
                    id: 'relay-access-key',
                    title: t('settings:live.relayAccessKey'),
                    value: '',
                    placeholder: t('settings:live.relayAccessKeyPlaceholder'),
                    save: async (next) => {
                      const trimmed = next.trim();
                      if (!trimmed) {
                        throw new Error(t('settings:live.relayAccessKeyEmpty'));
                      }
                      await state.updateSettings((old) => old, { relayAccessKey: trimmed });
                    },
                  })
                }
              />
              {relay.has_access_key ? (
                <SettingsNavRow
                  testID="relay-access-key-clear"
                  title={t('settings:live.relayClearAccessKey')}
                  showsChevron={false}
                  onPress={() =>
                    run(state.updateSettings((old) => old, { relayAccessKey: '' }))
                  }
                />
              ) : null}
            </Section>
          </>
        ) : null}
      </IosSheetForm>
    </IosSheetPage>
  );
}
