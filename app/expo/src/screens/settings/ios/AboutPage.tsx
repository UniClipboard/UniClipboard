import {
  HStack,
  Image,
  Link,
  RNHostView,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  font,
  foregroundStyle,
  frame,
  listRowBackground,
  padding,
} from '@expo/ui/swift-ui/modifiers';
import { nativeApplicationVersion, nativeBuildVersion } from 'expo-application';
import { useTranslation } from 'react-i18next';

import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { chevronColor, SettingsIconTile, settingsTileColors } from './common';

const PROJECT_HOME = 'https://github.com/UniClipboard/UniClipboard';

/** 关于页（官方像素猫品牌标，浅色/深色自适应边框与阴影 + 版本 + GitHub 项目主页） */
export function AboutPage() {
  const { t } = useTranslation('settings');
  const version = nativeApplicationVersion ?? '0.0.0';
  const versionText =
    nativeBuildVersion && nativeBuildVersion !== version && version !== '0.0.0'
      ? `${version} (${nativeBuildVersion})`
      : version;

  return (
    <IosSheetPage title={t('category.about')}>
      <IosSheetForm>
        <Section modifiers={[listRowBackground('clear')]}>
          <VStack spacing={6} modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 8 })]}>
            <RNHostView matchContents>
              <UnicBrandLogo size={84} borderRadius={20} />
            </RNHostView>
            <SwiftUIText
              testID="about-name"
              modifiers={[font({ size: 22, weight: 'bold' }), padding({ top: 6 })]}
            >
              UniClipboard
            </SwiftUIText>
            <SwiftUIText testID="about-version" modifiers={[foregroundStyle('secondary')]}>
              {`${t('ios.about.version')} ${versionText}`}
            </SwiftUIText>
          </VStack>
        </Section>

        <Section>
          <Link destination={PROJECT_HOME}>
            <HStack spacing={12} modifiers={[frame({ maxWidth: Infinity })]}>
              <SettingsIconTile systemName="globe" color={settingsTileColors.gray} />
              <SwiftUIText modifiers={[foregroundStyle('primary')]}>
                {t('ios.about.projectHome')}
              </SwiftUIText>
              <Spacer />
              <SwiftUIText modifiers={[foregroundStyle('secondary')]}>GitHub</SwiftUIText>
              <Image systemName="arrow.up.right" size={12} color={chevronColor} />
            </HStack>
          </Link>
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
