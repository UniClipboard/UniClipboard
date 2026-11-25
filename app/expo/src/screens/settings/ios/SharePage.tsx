import React from 'react';
import { useTranslation } from 'react-i18next';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  accessibilityElement,
  accessibilityHidden,
  background,
  controlSize,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  minimumScaleFactor,
  multilineTextAlignment,
  padding,
  shapes,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { SettingsIconTile, SettingsNavRow, settingsTileColors } from './common';
import { guideColors } from './SettingsGuideSheet';
import { ShareSheetMock, useOpenSampleShareSheet } from './ShareFavoritesGuideSheet';

const FLOW_STEPS: { key: string; icon: SFSymbol }[] = [
  { key: 'tapShare', icon: 'square.and.arrow.up' },
  { key: 'pickApp', icon: 'doc.on.clipboard' },
  { key: 'chooseDevices', icon: 'laptopcomputer.and.iphone' },
];

const CONTENT_TYPES: { key: string; icon: SFSymbol; color: string }[] = [
  { key: 'text', icon: 'text.alignleft', color: settingsTileColors.gray },
  { key: 'links', icon: 'link', color: settingsTileColors.blue },
  { key: 'images', icon: 'photo', color: settingsTileColors.orange },
  { key: 'files', icon: 'doc', color: settingsTileColors.indigo },
];

/** 分享扩展页（对齐 v1 SharePage.tsx） */
export function SharePage({ onOpenFavoritesGuide }: { onOpenFavoritesGuide: () => void }) {
  const { t } = useTranslation('settingsIos');
  const openShareSheet = useOpenSampleShareSheet();

  return (
    <IosSheetPage title={t('share.title')}>
      <IosSheetForm>
        <Section>
          <VStack
            alignment="leading"
            spacing={18}
            modifiers={[
              frame({ maxWidth: Infinity, alignment: 'leading' }),
              padding({ vertical: 8 }),
            ]}
          >
            <HStack
              modifiers={[
                frame({ maxWidth: Infinity }),
                padding({ vertical: 18 }),
                background(guideColors.canvas, shapes.roundedRectangle({ cornerRadius: 18 })),
                accessibilityHidden(true),
              ]}
            >
              <Spacer />
              <ShareSheetMock highlight="uniclip" />
              <Spacer />
            </HStack>
            <VStack alignment="leading" spacing={6}>
              <SwiftUIText modifiers={[font({ size: 22, weight: 'bold' })]}>
                {t('share.hero.title')}
              </SwiftUIText>
              <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
                {t('share.hero.description')}
              </SwiftUIText>
            </VStack>
            <HStack alignment="top" spacing={4} modifiers={[frame({ maxWidth: Infinity })]}>
              {FLOW_STEPS.map((step, index) => (
                <React.Fragment key={step.key}>
                  {index > 0 ? (
                    <Image
                      systemName="chevron.right"
                      size={12}
                      modifiers={[
                        foregroundStyle('tertiary'),
                        padding({ top: 14 }),
                        accessibilityHidden(true),
                      ]}
                    />
                  ) : null}
                  <VStack
                    spacing={6}
                    modifiers={[frame({ maxWidth: Infinity }), accessibilityElement('combine')]}
                  >
                    <Image
                      systemName={step.icon}
                      size={17}
                      color={settingsTileColors.blue}
                      modifiers={[
                        frame({ width: 40, height: 40 }),
                        background(guideColors.canvas, shapes.circle()),
                      ]}
                    />
                    <SwiftUIText
                      modifiers={[
                        font({ size: 13 }),
                        foregroundStyle('secondary'),
                        multilineTextAlignment('center'),
                      ]}
                    >
                      {t(`share.flow.${step.key}`)}
                    </SwiftUIText>
                  </VStack>
                </React.Fragment>
              ))}
            </HStack>
            <SwiftUIButton
              testID="share-try-now"
              onPress={openShareSheet}
              modifiers={[
                ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                controlSize('large'),
              ]}
            >
              <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {t('share.tryButton')}
                </SwiftUIText>
                <Image systemName="square.and.arrow.up" size={15} />
                <Spacer />
              </HStack>
            </SwiftUIButton>
          </VStack>
        </Section>

        <Section header={<SwiftUIText>{t('share.worksWith.title')}</SwiftUIText>}>
          <HStack spacing={0} modifiers={[frame({ maxWidth: Infinity }), padding({ vertical: 6 })]}>
            {CONTENT_TYPES.map((type) => (
              <VStack
                key={type.key}
                spacing={8}
                modifiers={[frame({ maxWidth: Infinity }), accessibilityElement('combine')]}
              >
                <SettingsIconTile systemName={type.icon} color={type.color} />
                <SwiftUIText
                  modifiers={[
                    font({ size: 13 }),
                    foregroundStyle('secondary'),
                    lineLimit(1),
                    minimumScaleFactor(0.8),
                  ]}
                >
                  {t(`share.worksWith.${type.key}`)}
                </SwiftUIText>
              </VStack>
            ))}
          </HStack>
        </Section>

        <Section
          header={<SwiftUIText>{t('share.quickAccess.title')}</SwiftUIText>}
          footer={<SwiftUIText>{t('share.quickAccess.footer')}</SwiftUIText>}
        >
          <SettingsNavRow
            testID="share-favorites-guide"
            icon="star.fill"
            iconColor={settingsTileColors.yellow}
            title={t('share.quickAccess.favoritesRow')}
            subtitle={t('share.quickAccess.favoritesRowSubtitle')}
            onPress={onOpenFavoritesGuide}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
