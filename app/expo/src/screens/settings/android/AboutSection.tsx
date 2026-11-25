/**
 * 关于二级页(Android) — 对齐原仓库 UniClip AboutSection.tsx M3 Expressive 设计，
 * 顶部居中 28dp 应用信息卡片使用官方像素猫品牌标（UnicBrandLogo）+ 版本号 + GitHub 项目主页入口。
 */
import { memo } from 'react';
import {
  Column,
  Icon,
  OutlinedButton,
  RNHostView,
  Spacer,
  Text as ComposeText,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  fillMaxWidth,
  height as heightModifier,
  size,
  testID,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';
import { nativeApplicationVersion, nativeBuildVersion } from 'expo-application';
import { useTranslation } from 'react-i18next';

import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { openWebLink } from '@/engine/historyActions';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsListRow } from './SettingsListRow';

const PROJECT_HOME = 'https://github.com/UniClipboard/UniClipboard';

const ICONS = {
  openInNew: require('../../../assets/icons/open_in_new.xml'),
  globe: require('../../../assets/icons/public.xml'),
};

const APP_ICON_SIZE = 84;
const APP_NAME_STYLE = { typography: 'titleLarge' } as const;

export const AboutSection = memo(function AboutSection() {
  const { t } = useTranslation();
  const colors = useMaterialColors();
  const version = nativeApplicationVersion ?? '0.0.0';
  const versionText =
    nativeBuildVersion && nativeBuildVersion !== version && version !== '0.0.0'
      ? `${version} (${nativeBuildVersion})`
      : version;

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard horizontalAlignment="center">
        <RNHostView matchContents modifiers={[size(APP_ICON_SIZE, APP_ICON_SIZE)]}>
          <UnicBrandLogo size={APP_ICON_SIZE} borderRadius={22} />
        </RNHostView>
        <Column horizontalAlignment="center">
          <ComposeText style={APP_NAME_STYLE} modifiers={[testID('about-name')]}>
            UniClipboard
          </ComposeText>
          <ComposeText color={colors.onSurfaceVariant} modifiers={[testID('about-version')]}>
            {t('settingsAbout:versionValue', { version: versionText })}
          </ComposeText>
        </Column>
        <OutlinedButton
          modifiers={[testID('about-github')]}
          onClick={() => void openWebLink(PROJECT_HOME)}
        >
          <Icon source={ICONS.openInNew} size={18} tint={colors.primary} />
          <Spacer modifiers={[widthModifier(8)]} />
          <ComposeText>GitHub</ComposeText>
        </OutlinedButton>
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="project-home"
          testID="about-project-home"
          icon={ICONS.globe}
          iconTone="muted"
          title={t('settings:ios.about.projectHome')}
          description="UniClipboard/UniClipboard"
          trailing={{ text: 'GitHub', chevron: true }}
          onPress={() => void openWebLink(PROJECT_HOME)}
        />
      </SettingsSectionItem>
    </Column>
  );
});
