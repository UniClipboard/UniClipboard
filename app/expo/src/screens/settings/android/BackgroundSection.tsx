/**
 * 后台运行二级页(Android) — 对齐原仓库 UniClip BackgroundSection.tsx M3 Expressive 设计：
 * 顶部主开关使用 28dp SettingsHeroCard（开启时 primaryContainer 强调，contentPadding=8 贴合 ListItem），
 * 其下 grouped 分组提供系统运行保障入口。
 */
import { memo, useCallback, useRef, useState } from 'react';
import { Alert, Linking } from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import { Column, Spacer } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, height as heightModifier } from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';

import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const ICONS = {
  settings: require('../../../assets/icons/settings.xml'),
};

export const BackgroundSection = memo(function BackgroundSection() {
  const { t } = useTranslation();
  const [value, setValue] = useState<boolean | null>(null);
  const busy = useRef(false);

  useFocusEffect(
    useCallback(() => {
      let current = true;
      void unicEngineNative
        .getBackgroundSyncEnabled()
        .then((enabled) => {
          if (current) setValue(enabled);
        })
        .catch(() => {
          if (current) setValue(null);
        });
      return () => {
        current = false;
      };
    }, [])
  );

  const handleToggle = (enabled: boolean) => {
    if (busy.current) return;
    busy.current = true;
    void unicEngineNative
      .setBackgroundSyncEnabled(enabled)
      .then(setValue)
      .catch((error) => {
        Alert.alert(
          t('settingsStorage:setFailed'),
          t(`errors:engine.${parseEngineError(error).code}`, {
            defaultValue: t('errors:engine.internal'),
          })
        );
      })
      .finally(() => {
        busy.current = false;
      });
  };

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard tone={value ? 'primary' : 'neutral'} contentPadding={8}>
        {value === null ? (
          <SettingsListRow
            testID="background-sync-loading"
            title={t('settings:live.loading')}
          />
        ) : (
          <SettingsSwitchRow
            testID="background-sync"
            title={t('settingsBackground:main.cardTitle')}
            description={t('settingsBackground:live.description')}
            value={value}
            onValueChange={handleToggle}
          />
        )}
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(20)]} />

      <SettingsSectionItem
        variant="grouped"
        title={t('settingsBackground:advanced.groups.reliability')}
        footer={t('settingsBackground:main.footer')}
      >
        <SettingsListRow
          key="background-system-settings"
          testID="background-system-settings"
          icon={ICONS.settings}
          iconTone="muted"
          title={t('settings:openSystemSettings')}
          description={t('settingsBackground:advanced.battery.desc')}
          trailing="chevron"
          onPress={() => void Linking.openSettings()}
        />
      </SettingsSectionItem>
    </Column>
  );
});
