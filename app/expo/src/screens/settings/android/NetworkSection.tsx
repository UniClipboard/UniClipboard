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
  const runtime = state.snapshot?.runtime;
  const isActive = runtime === 'active';
  const runtimeText = runtime ? t(`settings:live.status.${runtime}`) : '—';
  const connectedText = t('settings:live.connected', { connected, total: bindings.length });

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
        </>
      ) : null}
    </Column>
  );
});
