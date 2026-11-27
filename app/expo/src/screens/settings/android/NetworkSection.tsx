/**
 * 高级网络二级页(Android) — M3 Expressive：
 * 顶部状态概览卡（网络运行状态 + Tailcat 连接计数）→ 运行状态只读明细 → 配对服务 URL 配置。
 */
import { memo, useCallback } from 'react';
import { Alert } from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import {
  Column,
  Row,
  Spacer,
  Text as ComposeText,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  fillMaxWidth,
  height as heightModifier,
  weight,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { useEngineStore } from '@/engine/store';
import { useTheme } from '@/hooks/useTheme';
import { SettingsSectionItem } from '../SettingsSectionItem';
import type { AndroidSettingFieldEdit } from './SettingsFieldDialog';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsLeadingIcon } from './SettingsLeadingIcon';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSelectRow } from './SettingsSelectRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const DEFAULT_RELAY_MAX_MB = 25;
const RELAY_MAX_MB_OPTIONS = [10, 25, 50, 100, 250];

const ICONS = {
  network: require('../../../assets/icons/wifi.xml'),
};

const HERO_TITLE_STYLE = { typography: 'titleLarge' } as const;

export const NetworkSection = memo(function NetworkSection({
  onEdit,
}: {
  onEdit: (edit: AndroidSettingFieldEdit) => void;
}) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const colors = useMaterialColors();
  const state = useEngineStore(
    useShallow((s) => ({
      snapshot: s.snapshot,
      devices: s.devices,
      updateSettings: s.updateSettings,
    }))
  );

  useFocusEffect(
    useCallback(() => {
      void useEngineStore.getState().refreshSnapshot();
    }, [])
  );

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  const settings = state.snapshot?.settings.value;
  const bindings = state.devices
    .flatMap((device) => device.bindings ?? [])
    .filter((binding) => binding.instance_id === 'builtin.tailcat');
  const connected = bindings.filter((binding) => binding.connection === 'connected').length;
  const relayReady = bindings.filter((binding) => Boolean(binding.relay_route_ready)).length;
  const runtime = state.snapshot?.runtime;
  const isActive = runtime === 'active';
  const runtimeText = runtime ? t(`settings:live.status.${runtime}`) : '—';
  const connectedText = t('settings:live.connected', { connected, total: bindings.length });
  const relayReadyText = t('settings:live.relayReadyCount', { ready: relayReady, total: bindings.length });
  const relay = settings?.sync.relay ?? { enabled: false };
  const relayMaxMB = relay.max_bytes && relay.max_bytes > 0
    ? Math.max(1, Math.round(relay.max_bytes / (1024 * 1024)))
    : DEFAULT_RELAY_MAX_MB;
  const relayMaxPresets = RELAY_MAX_MB_OPTIONS.includes(relayMaxMB)
    ? RELAY_MAX_MB_OPTIONS
    : [...RELAY_MAX_MB_OPTIONS, relayMaxMB].sort((a, b) => a - b);
  const relayMaxOptions = relayMaxPresets.map((mb) => ({
    value: String(mb),
    label: mb === DEFAULT_RELAY_MAX_MB ? `${mb} MB (${t('settings:live.default')})` : `${mb} MB`,
  }));

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard tone={isActive ? 'primary' : 'neutral'}>
        <Row verticalAlignment="center" modifiers={[fillMaxWidth()]}>
          <SettingsLeadingIcon source={ICONS.network} tone={isActive ? 'accent' : 'muted'} />
          <Spacer modifiers={[widthModifier(14)]} />
          <Column modifiers={[weight(1)]}>
            <ComposeText style={HERO_TITLE_STYLE}>{runtimeText}</ComposeText>
            <ComposeText color={isActive ? colors.onPrimaryContainer : colors.onSurfaceVariant}>
              {`${t('settings:live.tailcat')} · ${connectedText}`}
            </ComposeText>
            {relay.enabled ? (
              <ComposeText color={isActive ? colors.onPrimaryContainer : colors.onSurfaceVariant}>
                {`${t('settings:live.relaySection')} · ${relayReadyText}`}
              </ComposeText>
            ) : null}
          </Column>
        </Row>
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="network-runtime"
          testID="network-runtime"
          title={t('settings:live.runtime')}
          trailing={{
            text: runtimeText,
            color: isActive ? (theme.colors.success as string) : (theme.colors.warning as string),
          }}
        />
        <SettingsListRow
          key="network-tailcat"
          testID="network-tailcat"
          title={t('settings:live.tailcat')}
          trailing={{ text: connectedText }}
        />
        <SettingsListRow
          key="network-relay-ready"
          testID="network-relay-ready"
          title={t('settings:live.relayReadyTitle')}
          trailing={{
            text: relayReadyText,
            color: relay.enabled && relayReady > 0 ? (theme.colors.success as string) : undefined,
          }}
        />
      </SettingsSectionItem>

      {settings ? (
        <>
          <Spacer modifiers={[heightModifier(16)]} />
          <SettingsSectionItem variant="grouped">
            <SettingsListRow
              key="rendezvous-url"
              testID="rendezvous-url"
              title={t('settings:live.rendezvous')}
              trailing={{
                text: settings.rendezvous_url || t('settings:live.default'),
                chevron: true,
              }}
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
            <SettingsListRow
              key="rendezvous-reset"
              testID="rendezvous-reset"
              title={t('settings:live.resetRendezvous')}
              trailing={{ action: t('common:action.reset', { defaultValue: t('settings:live.default') }) }}
              disabled={!settings.rendezvous_url}
              onPress={() =>
                run(state.updateSettings((old) => ({ ...old, rendezvous_url: '' })))
              }
            />
          </SettingsSectionItem>

          <Spacer modifiers={[heightModifier(16)]} />
          <SettingsSectionItem
            title={t('settings:live.relaySection')}
            footer={t('settings:live.relayFooter')}
            variant="grouped"
          >
            <SettingsSwitchRow
              key="relay-enabled"
              testID="relay-enabled"
              title={t('settings:live.relayEnabled')}
              description={t('settings:live.relayEnabledDesc')}
              value={Boolean(relay.enabled)}
              onValueChange={(enabled) =>
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
            <SettingsListRow
              key="relay-url"
              testID="relay-url"
              title={t('settings:live.relayUrl')}
              trailing={{
                text: relay.url || t('settings:live.relayDefault'),
                chevron: true,
              }}
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
              <SettingsListRow
                key="relay-url-reset"
                testID="relay-url-reset"
                title={t('settings:live.relayResetUrl')}
                trailing={{ action: t('common:action.reset', { defaultValue: t('settings:live.default') }) }}
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
            <SettingsSelectRow
              key="relay-max-bytes"
              testID="relay-max-bytes"
              title={t('settings:live.relayMaxSize')}
              options={relayMaxOptions}
              selectedValue={String(relayMaxMB)}
              onSelect={(value) => {
                const mb = Number.parseInt(value, 10);
                if (!Number.isFinite(mb) || mb <= 0) return;
                run(
                  state.updateSettings((old) => ({
                    ...old,
                    sync: {
                      ...old.sync,
                      relay: {
                        ...(old.sync.relay ?? { enabled: false }),
                        max_bytes: mb * 1024 * 1024,
                      },
                    },
                  }))
                );
              }}
            />
            <SettingsListRow
              key="relay-access-key"
              testID="relay-access-key"
              title={t('settings:live.relayAccessKey')}
              trailing={{
                text: relay.has_access_key
                  ? t('settings:live.relayKeyConfigured')
                  : t('settings:live.relayKeyNotSet'),
                color: relay.has_access_key ? (theme.colors.success as string) : undefined,
                chevron: true,
              }}
              onPress={() =>
                onEdit({
                  id: 'relay-access-key',
                  title: t('settings:live.relayAccessKey'),
                  value: '',
                  placeholder: t('settings:live.relayAccessKeyPlaceholder'),
                  secure: true,
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
              <SettingsListRow
                key="relay-access-key-clear"
                testID="relay-access-key-clear"
                title={t('settings:live.relayClearAccessKey')}
                destructive
                trailing={{ action: t('common:action.clear', { defaultValue: t('settings:live.relayClearAccessKey') }) }}
                onPress={() =>
                  run(state.updateSettings((old) => old, { relayAccessKey: '' }))
                }
              />
            ) : null}
          </SettingsSectionItem>
        </>
      ) : null}
    </Column>
  );
});
