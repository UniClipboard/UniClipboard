/**
 * 剪贴板访问权限二级页(Android) — M3 Expressive：
 * 顶部状态概览卡 + 实时读取/写入能力只读行（无误导性下拉箭头）+ 系统设置直达入口。
 */
import { memo, useCallback } from 'react';
import { Linking } from 'react-native';
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

import { useEngineStore } from '@/engine/store';
import { useTheme } from '@/hooks/useTheme';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsLeadingIcon } from './SettingsLeadingIcon';
import { SettingsListRow } from './SettingsListRow';

const ICONS = {
  clipboard: require('../../../assets/icons/content_paste.xml'),
  settings: require('../../../assets/icons/settings.xml'),
};

const HERO_TITLE_STYLE = { typography: 'titleLarge' } as const;

export const ClipboardAccessSection = memo(function ClipboardAccessSection() {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const colors = useMaterialColors();
  const capabilities = useEngineStore((s) => s.capabilities);

  useFocusEffect(
    useCallback(() => {
      void useEngineStore.getState().refreshSnapshot();
    }, [])
  );

  const observeState = capabilities?.effective.observe;
  const applyState = capabilities?.effective.apply;
  const observeAvailable = observeState === 'available';
  const applyAvailable = applyState === 'available';
  const allReady = observeAvailable && applyAvailable;

  const observeText = observeState ? t(`settings:live.status.${observeState}`) : '—';
  const applyText = applyState ? t(`settings:live.status.${applyState}`) : '—';
  const restrictions = capabilities?.restrictions ?? [];

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard tone={allReady ? 'primary' : 'neutral'}>
        <Row verticalAlignment="center" modifiers={[fillMaxWidth()]}>
          <SettingsLeadingIcon source={ICONS.clipboard} tone={allReady ? 'accent' : 'primary'} />
          <Spacer modifiers={[widthModifier(14)]} />
          <Column modifiers={[weight(1)]}>
            <ComposeText style={HERO_TITLE_STYLE}>
              {t('settings:ios.extensions.clipboardAccess')}
            </ComposeText>
            <ComposeText color={allReady ? colors.onPrimaryContainer : colors.onSurfaceVariant}>
              {`${t('settings:live.observe')}：${observeText} · ${t('settings:live.apply')}：${applyText}`}
            </ComposeText>
          </Column>
        </Row>
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="clipboard-observe"
          testID="clipboard-observe"
          title={t('settings:live.observe')}
          trailing={{
            text: observeText,
            color: observeAvailable
              ? (theme.colors.success as string)
              : (theme.colors.warning as string),
          }}
        />
        <SettingsListRow
          key="clipboard-apply"
          testID="clipboard-apply"
          title={t('settings:live.apply')}
          trailing={{
            text: applyText,
            color: applyAvailable
              ? (theme.colors.success as string)
              : (theme.colors.warning as string),
          }}
        />
        {restrictions.map((reason) => (
          <SettingsListRow
            key={reason}
            testID={`restriction-${reason}`}
            title={t(`settings:live.status.${reason}`, { defaultValue: reason })}
          />
        ))}
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="clipboard-system-settings"
          testID="clipboard-system-settings"
          icon={ICONS.settings}
          iconTone="muted"
          title={t('settings:openSystemSettings')}
          trailing="chevron"
          onPress={() => void Linking.openSettings()}
        />
      </SettingsSectionItem>
    </Column>
  );
});
