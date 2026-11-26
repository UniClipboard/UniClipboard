import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Capsule,
  Divider,
  HStack,
  Image,
  RoundedRectangle,
  Spacer,
  Text as SwiftUIText,
  VStack,
  ZStack,
} from '@expo/ui/swift-ui';
import {
  accessibilityHidden,
  background,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  minimumScaleFactor,
  padding,
  shadow,
  shapes,
  strokeBorder,
  textCase,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import { unicEngineNative } from '@/engine/nativeModule';
import { settingsTileColors } from './common';
import {
  GUIDE_CANVAS_HEIGHT,
  GuideStepPage,
  GuideTileIcon,
  SettingsGuideSheet,
  guideColors,
  type GuideColor,
} from './SettingsGuideSheet';

export interface ShareFavoritesGuideSheetProps {
  visible: boolean;
  onClose: () => void;
}

const GUIDE_STEPS = ['openSheet', 'findMore', 'addFavorite', 'moveToTop'] as const;
type GuideStep = (typeof GUIDE_STEPS)[number];

const APP_NAME = 'Unic';

const APP_COLORS = {
  uniclip: settingsTileColors.blue,
  airDrop: '#6E9FE0',
  messages: '#8FD19E',
  notes: '#F7C873',
} as const;

const EDIT_ROW_HEIGHT = 40;

/** Opens a real iOS share sheet via the native Engine module with a sample message. */
export function useOpenSampleShareSheet() {
  const { t } = useTranslation('settingsIos');
  return useCallback(() => {
    void unicEngineNative.shareHistoryMedia([], t('share.testMessage')).catch(() => {
      // user dismissed or native share unavailable
    });
  }, [t]);
}

export function ShareFavoritesGuideSheet({ visible, onClose }: ShareFavoritesGuideSheetProps) {
  const { t } = useTranslation('settingsIos');
  const openShareSheet = useOpenSampleShareSheet();

  return (
    <SettingsGuideSheet
      visible={visible}
      onClose={onClose}
      testIDPrefix="share-guide"
      title={t('share.guide.title')}
      steps={GUIDE_STEPS}
      renderStep={(step) => (
        <GuideStepPage
          illustration={<GuideIllustration step={step} />}
          title={t(`share.guide.steps.${step}.title`)}
          description={t(`share.guide.steps.${step}.description`)}
        />
      )}
      primaryAction={{
        label: t('share.guide.openShareSheet'),
        systemImage: 'square.and.arrow.up',
        onPress: openShareSheet,
      }}
      labels={{
        progress: (current, total) => t('share.guide.progress', { current, total }),
        back: t('share.guide.back'),
        close: t('share.guide.close'),
        next: t('share.guide.next'),
        done: t('share.guide.done'),
      }}
    />
  );
}

function GuideIllustration({ step }: { step: GuideStep }) {
  const { t } = useTranslation('settingsIos');
  const label = (key: string) => t(`share.guide.illustration.${key}`);

  if (step === 'openSheet') {
    return (
      <GuideCanvas>
        <NoteMock />
      </GuideCanvas>
    );
  }
  if (step === 'findMore') {
    return (
      <GuideCanvas>
        <ShareSheetMock highlight="more" />
      </GuideCanvas>
    );
  }
  if (step === 'addFavorite') {
    return (
      <GuideCanvas alignment="topLeading">
        <AppListMock
          favorites={[
            { name: label('messages'), color: APP_COLORS.messages },
            { name: label('notes'), color: APP_COLORS.notes },
          ]}
          suggestions={[
            {
              name: APP_NAME,
              color: APP_COLORS.uniclip,
              icon: 'doc.on.clipboard',
              highlighted: true,
            },
            { name: label('reminders'), color: guideColors.neutralTile },
          ]}
        />
      </GuideCanvas>
    );
  }
  return (
    <GuideCanvas alignment="topLeading">
      <AppListMock
        favorites={[
          {
            name: APP_NAME,
            color: APP_COLORS.uniclip,
            icon: 'doc.on.clipboard',
            highlighted: true,
          },
          { name: label('messages'), color: APP_COLORS.messages },
          { name: label('notes'), color: APP_COLORS.notes },
        ]}
        suggestions={[{ name: label('reminders'), color: guideColors.neutralTile }]}
      />
    </GuideCanvas>
  );
}

function GuideCanvas({
  alignment = 'center',
  children,
}: {
  alignment?: 'center' | 'topLeading';
  children: React.ReactNode;
}) {
  return (
    <ZStack
      alignment={alignment}
      modifiers={[
        frame({ maxWidth: Infinity, height: GUIDE_CANVAS_HEIGHT }),
        strokeBorder({
          content: guideColors.border,
          style: { lineWidth: 0.5 },
          shape: 'roundedRectangle',
          cornerRadius: 28,
        }),
        accessibilityHidden(true),
      ]}
    >
      <RoundedRectangle cornerRadius={28} modifiers={[foregroundStyle(guideColors.canvas)]} />
      <VStack
        modifiers={[
          frame({
            maxWidth: Infinity,
            maxHeight: Infinity,
            alignment: alignment === 'center' ? 'center' : 'topLeading',
          }),
          padding({ all: 14 }),
        ]}
      >
        {children}
      </VStack>
    </ZStack>
  );
}

function NoteMock() {
  const { t } = useTranslation('settingsIos');
  return (
    <VStack
      alignment="leading"
      spacing={10}
      modifiers={[
        frame({ width: 290 }),
        padding({ all: 16 }),
        background(guideColors.card, shapes.roundedRectangle({ cornerRadius: 22 })),
        shadow({ radius: 14, y: 8, color: '#0000001F' }),
      ]}
    >
      <SwiftUIText modifiers={[font({ size: 17, weight: 'bold' }), lineLimit(1)]}>
        {t('share.guide.illustration.noteTitle')}
      </SwiftUIText>
      {[240, 210, 225, 140].map((width) => (
        <Capsule
          key={width}
          modifiers={[foregroundStyle(guideColors.placeholder), frame({ width, height: 8 })]}
        />
      ))}
      <Divider />
      <HStack modifiers={[frame({ maxWidth: Infinity })]}>
        <Image systemName="chevron.backward" size={17} modifiers={[foregroundStyle('secondary')]} />
        <Spacer />
        <ZStack
          modifiers={[
            frame({ width: 40, height: 40 }),
            background(guideColors.highlightFill, shapes.circle()),
            strokeBorder({
              content: guideColors.blue,
              style: { lineWidth: 2 },
              shape: 'circle',
            }),
          ]}
        >
          <Image systemName="square.and.arrow.up" size={17} color={guideColors.blue} />
        </ZStack>
        <Spacer />
        <Image systemName="star" size={17} modifiers={[foregroundStyle('secondary')]} />
        <Spacer />
        <Image systemName="ellipsis" size={17} modifiers={[foregroundStyle('secondary')]} />
      </HStack>
    </VStack>
  );
}

export function ShareSheetMock({ highlight }: { highlight: 'uniclip' | 'more' }) {
  const { t } = useTranslation('settingsIos');
  const label = (key: string) => t(`share.guide.illustration.${key}`);
  return (
    <VStack
      spacing={14}
      modifiers={[
        frame({ width: 290 }),
        padding({ all: 14 }),
        background(guideColors.card, shapes.roundedRectangle({ cornerRadius: 26 })),
        shadow({ radius: 14, y: 8, color: '#0000001F' }),
        accessibilityHidden(true),
      ]}
    >
      <HStack spacing={10} modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}>
        <Image
          systemName="text.alignleft"
          size={15}
          modifiers={[
            foregroundStyle('secondary'),
            frame({ width: 36, height: 36 }),
            background(guideColors.canvas, shapes.roundedRectangle({ cornerRadius: 8 })),
          ]}
        />
        <VStack alignment="leading" spacing={1}>
          <SwiftUIText modifiers={[font({ size: 13, weight: 'semibold' }), lineLimit(1)]}>
            {label('noteTitle')}
          </SwiftUIText>
          <SwiftUIText modifiers={[font({ size: 11 }), foregroundStyle('secondary'), lineLimit(1)]}>
            {label('noteSubtitle')}
          </SwiftUIText>
        </VStack>
        <Spacer />
      </HStack>
      <Divider />
      <HStack modifiers={[frame({ maxWidth: Infinity })]}>
        <ShareAppTile name={label('airDrop')} color={APP_COLORS.airDrop} />
        <Spacer />
        <ShareAppTile
          name={APP_NAME}
          color={APP_COLORS.uniclip}
          icon="doc.on.clipboard"
          highlighted={highlight === 'uniclip'}
        />
        <Spacer />
        <ShareAppTile name={label('messages')} color={APP_COLORS.messages} />
        <Spacer />
        {highlight === 'more' ? (
          <ShareAppTile
            name={label('more')}
            color={guideColors.placeholder}
            icon="ellipsis"
            iconColor="secondary"
            highlighted
          />
        ) : (
          <ShareAppTile name={label('notes')} color={APP_COLORS.notes} />
        )}
      </HStack>
    </VStack>
  );
}

function ShareAppTile({
  name,
  color,
  icon,
  iconColor,
  highlighted = false,
}: {
  name: string;
  color: GuideColor;
  icon?: SFSymbol;
  iconColor?: 'secondary';
  highlighted?: boolean;
}) {
  return (
    <VStack spacing={4} modifiers={[frame({ width: 58 })]}>
      <ZStack
        modifiers={[
          frame({ width: 56, height: 56 }),
          ...(highlighted
            ? [
                strokeBorder({
                  content: guideColors.blue,
                  style: { lineWidth: 2 },
                  shape: 'roundedRectangle',
                  cornerRadius: 14,
                }),
              ]
            : []),
        ]}
      >
        <GuideTileIcon
          icon={iconColor ? undefined : icon}
          color={color}
          size={48}
          cornerRadius={11}
        />
        {icon && iconColor ? (
          <Image systemName={icon} size={20} modifiers={[foregroundStyle(iconColor)]} />
        ) : null}
      </ZStack>
      <SwiftUIText
        modifiers={[
          font({ size: 11, weight: highlighted ? 'semibold' : 'regular' }),
          foregroundStyle(highlighted ? 'primary' : 'secondary'),
          lineLimit(1),
          minimumScaleFactor(0.7),
        ]}
      >
        {name}
      </SwiftUIText>
    </VStack>
  );
}

interface AppListEntry {
  name: string;
  color: GuideColor;
  icon?: SFSymbol;
  highlighted?: boolean;
}

function AppListMock({
  favorites,
  suggestions,
}: {
  favorites: AppListEntry[];
  suggestions: AppListEntry[];
}) {
  const { t } = useTranslation('settingsIos');
  const label = (key: string) => t(`share.guide.illustration.${key}`);
  return (
    <VStack alignment="leading" spacing={6} modifiers={[frame({ maxWidth: Infinity })]}>
      <HStack modifiers={[frame({ maxWidth: Infinity, height: 24 })]}>
        <SwiftUIText modifiers={[font({ size: 15, weight: 'semibold' })]}>
          {label('apps')}
        </SwiftUIText>
        <Spacer />
        <SwiftUIText
          modifiers={[font({ size: 15, weight: 'semibold' }), foregroundStyle(guideColors.blue)]}
        >
          {label('done')}
        </SwiftUIText>
      </HStack>
      <AppListSectionLabel text={label('favorites')} />
      <AppListGroup>
        {favorites.map((entry, index) => (
          <React.Fragment key={entry.name}>
            {index > 0 ? <Divider /> : null}
            <AppListRow entry={entry} control="remove" />
          </React.Fragment>
        ))}
      </AppListGroup>
      <AppListSectionLabel text={label('suggestions')} />
      <AppListGroup>
        {suggestions.map((entry, index) => (
          <React.Fragment key={entry.name}>
            {index > 0 ? <Divider /> : null}
            <AppListRow entry={entry} control="add" />
          </React.Fragment>
        ))}
      </AppListGroup>
    </VStack>
  );
}

function AppListSectionLabel({ text }: { text: string }) {
  return (
    <SwiftUIText
      modifiers={[
        font({ size: 11, weight: 'semibold' }),
        foregroundStyle('secondary'),
        textCase('uppercase'),
        lineLimit(1),
        padding({ horizontal: 12, top: 4 }),
      ]}
    >
      {text}
    </SwiftUIText>
  );
}

function AppListGroup({ children }: { children: React.ReactNode }) {
  return (
    <VStack
      spacing={0}
      modifiers={[
        frame({ maxWidth: Infinity }),
        padding({ horizontal: 4, vertical: 2 }),
        background(guideColors.card, shapes.roundedRectangle({ cornerRadius: 14 })),
      ]}
    >
      {children}
    </VStack>
  );
}

function AppListRow({ entry, control }: { entry: AppListEntry; control: 'add' | 'remove' }) {
  const row = (
    <HStack
      spacing={10}
      modifiers={[
        frame({ maxWidth: Infinity, height: EDIT_ROW_HEIGHT }),
        padding({ horizontal: 8 }),
      ]}
    >
      <Image
        systemName={control === 'add' ? 'plus.circle.fill' : 'minus.circle.fill'}
        size={20}
        color={control === 'add' ? settingsTileColors.green : settingsTileColors.red}
      />
      <GuideTileIcon icon={entry.icon} color={entry.color} size={24} />
      <SwiftUIText
        modifiers={[
          font({
            size: 15,
            weight: entry.highlighted ? 'semibold' : 'regular',
          }),
          lineLimit(1),
          minimumScaleFactor(0.7),
        ]}
      >
        {entry.name}
      </SwiftUIText>
      <Spacer />
      {control === 'remove' ? (
        <Image systemName="line.3.horizontal" size={15} modifiers={[foregroundStyle('tertiary')]} />
      ) : null}
    </HStack>
  );
  if (!entry.highlighted) return row;
  return (
    <ZStack
      modifiers={[
        frame({ maxWidth: Infinity, height: EDIT_ROW_HEIGHT + 4 }),
        strokeBorder({
          content: guideColors.blue,
          style: { lineWidth: 2 },
          shape: 'roundedRectangle',
          cornerRadius: 11,
        }),
      ]}
    >
      <RoundedRectangle
        cornerRadius={11}
        modifiers={[foregroundStyle(guideColors.highlightFill)]}
      />
      {row}
    </ZStack>
  );
}
