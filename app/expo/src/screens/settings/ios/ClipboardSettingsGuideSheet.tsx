import React from 'react';
import { Linking } from 'react-native';
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
  background,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  minimumScaleFactor,
  padding,
  shapes,
  strokeBorder,
  textCase,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import {
  GUIDE_CANVAS_HEIGHT,
  GuideStepPage,
  GuideTapIndicator,
  GuideTileIcon,
  SettingsGuideSheet,
  guideColors,
  type GuideColor,
} from './SettingsGuideSheet';

export interface ClipboardSettingsGuideSheetProps {
  visible: boolean;
  onClose: () => void;
}

const GUIDE_STEPS = ['findApp', 'openPaste', 'chooseAllow'] as const;
type GuideStep = (typeof GUIDE_STEPS)[number];

const APP_NAME = 'UniClipboard';
const MOCK_ROW_HEIGHT = 40;

export function ClipboardSettingsGuideSheet({
  visible,
  onClose,
}: ClipboardSettingsGuideSheetProps) {
  const { t } = useTranslation('settingsPermissions');

  return (
    <SettingsGuideSheet
      visible={visible}
      onClose={onClose}
      testIDPrefix="clipboard-guide"
      title={t('clipboardAccess.guide.title')}
      steps={GUIDE_STEPS}
      renderStep={(step) => (
        <GuideStepPage
          illustration={<GuideIllustration step={step} />}
          title={t(`clipboardAccess.guide.steps.${step}.title`)}
          description={t(`clipboardAccess.guide.steps.${step}.description`)}
          tip={{
            tone: step === 'openPaste' ? 'warning' : 'info',
            text: t(`clipboardAccess.guide.steps.${step}.tip`),
          }}
        />
      )}
      primaryAction={{
        label: t('clipboardAccess.guide.openSettings'),
        systemImage: 'arrow.up.forward',
        onPress: () => {
          void Linking.openSettings();
        },
      }}
      labels={{
        progress: (current, total) => t('clipboardAccess.guide.progress', { current, total }),
        back: t('clipboardAccess.guide.back'),
        close: t('clipboardAccess.guide.close'),
        next: t('clipboardAccess.guide.next'),
        done: t('clipboardAccess.guide.done'),
      }}
    />
  );
}

function GuideIllustration({ step }: { step: GuideStep }) {
  const { t } = useTranslation('settingsPermissions');
  const label = (key: string) => t(`clipboardAccess.guide.illustration.${key}`);

  if (step === 'findApp') {
    return (
      <MockScreen backLabel={label('settings')} title={label('apps')}>
        <HStack
          spacing={6}
          modifiers={[
            frame({ maxWidth: Infinity, height: 32, alignment: 'leading' }),
            padding({ horizontal: 10 }),
            background(guideColors.placeholder, shapes.capsule()),
          ]}
        >
          <Image
            systemName="magnifyingglass"
            size={13}
            modifiers={[foregroundStyle('secondary')]}
          />
          <SwiftUIText modifiers={[font({ size: 14 }), foregroundStyle('secondary')]}>
            {label('search')}
          </SwiftUIText>
          <Spacer />
        </HStack>
        <MockGroup>
          <MockPlaceholderRow width={72} />
          <Divider />
          <MockPlaceholderRow width={96} />
          <Divider />
          <MockHighlightedRow
            icon="doc.on.clipboard"
            iconColor={guideColors.blue}
            title={APP_NAME}
            chevron
          />
          <Divider />
          <MockPlaceholderRow width={64} />
        </MockGroup>
      </MockScreen>
    );
  }

  if (step === 'openPaste') {
    return (
      <MockScreen backLabel={label('apps')} title={APP_NAME}>
        <VStack alignment="leading" spacing={6}>
          <SwiftUIText
            modifiers={[
              font({ size: 12 }),
              foregroundStyle('secondary'),
              textCase('uppercase'),
              lineLimit(1),
              padding({ horizontal: 12 }),
            ]}
          >
            {label('accessHeader')}
          </SwiftUIText>
          <MockGroup>
            <MockPlaceholderRow width={48} />
            <Divider />
            <MockPlaceholderRow width={80} />
            <Divider />
            <MockHighlightedRow
              icon="doc.on.clipboard"
              iconColor={guideColors.neutralTile}
              title={label('pasteFromOtherApps')}
              value={label('ask')}
              chevron
            />
          </MockGroup>
        </VStack>
      </MockScreen>
    );
  }

  return (
    <MockScreen backLabel={APP_NAME} title={label('pasteFromOtherApps')}>
      <MockGroup>
        <MockTextRow title={label('ask')} />
        <Divider />
        <MockTextRow title={label('deny')} />
        <Divider />
        <MockHighlightedRow title={label('allow')} checked />
      </MockGroup>
    </MockScreen>
  );
}

function MockScreen({
  backLabel,
  title,
  children,
}: {
  backLabel: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <ZStack
      alignment="topLeading"
      modifiers={[
        frame({ maxWidth: Infinity, height: GUIDE_CANVAS_HEIGHT }),
        strokeBorder({
          content: guideColors.border,
          style: { lineWidth: 0.5 },
          shape: 'roundedRectangle',
          cornerRadius: 24,
        }),
      ]}
    >
      <RoundedRectangle cornerRadius={24} modifiers={[foregroundStyle(guideColors.canvas)]} />
      <VStack
        alignment="leading"
        spacing={10}
        modifiers={[
          frame({
            maxWidth: Infinity,
            maxHeight: Infinity,
            alignment: 'topLeading',
          }),
          padding({ all: 14 }),
        ]}
      >
        <HStack spacing={3}>
          <Image systemName="chevron.backward" size={12} color={guideColors.blue} />
          <SwiftUIText
            modifiers={[font({ size: 14 }), foregroundStyle(guideColors.blue), lineLimit(1)]}
          >
            {backLabel}
          </SwiftUIText>
        </HStack>
        <SwiftUIText
          modifiers={[font({ size: 24, weight: 'bold' }), lineLimit(1), minimumScaleFactor(0.6)]}
        >
          {title}
        </SwiftUIText>
        {children}
      </VStack>
    </ZStack>
  );
}

function MockGroup({ children }: { children: React.ReactNode }) {
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

function MockPlaceholderRow({ width }: { width: number }) {
  return (
    <HStack
      spacing={10}
      modifiers={[
        frame({ maxWidth: Infinity, height: MOCK_ROW_HEIGHT }),
        padding({ horizontal: 8 }),
      ]}
    >
      <GuideTileIcon color={guideColors.neutralTile} />
      <Capsule modifiers={[foregroundStyle(guideColors.placeholder), frame({ width, height: 9 })]} />
      <Spacer />
      <Image systemName="chevron.right" size={11} modifiers={[foregroundStyle('tertiary')]} />
    </HStack>
  );
}

function MockTextRow({ title }: { title: string }) {
  return (
    <HStack
      modifiers={[
        frame({ maxWidth: Infinity, height: MOCK_ROW_HEIGHT }),
        padding({ horizontal: 10 }),
      ]}
    >
      <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
        {title}
      </SwiftUIText>
      <Spacer />
    </HStack>
  );
}

function MockHighlightedRow({
  icon,
  iconColor,
  title,
  value,
  chevron = false,
  checked = false,
}: {
  icon?: SFSymbol;
  iconColor?: GuideColor;
  title: string;
  value?: string;
  chevron?: boolean;
  checked?: boolean;
}) {
  return (
    <ZStack
      modifiers={[
        frame({ maxWidth: Infinity, height: MOCK_ROW_HEIGHT + 4 }),
        strokeBorder({
          content: guideColors.blue,
          style: { lineWidth: 2 },
          shape: 'roundedRectangle',
          cornerRadius: 11,
        }),
      ]}
    >
      <RoundedRectangle cornerRadius={11} modifiers={[foregroundStyle(guideColors.highlightFill)]} />
      <HStack spacing={10} modifiers={[frame({ maxWidth: Infinity }), padding({ horizontal: 8 })]}>
        {icon && iconColor ? <GuideTileIcon icon={icon} color={iconColor} /> : null}
        <SwiftUIText
          modifiers={[
            font({ size: 15, weight: 'semibold' }),
            lineLimit(1),
            minimumScaleFactor(0.7),
          ]}
        >
          {title}
        </SwiftUIText>
        <Spacer />
        {value ? (
          <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary'), lineLimit(1)]}>
            {value}
          </SwiftUIText>
        ) : null}
        <GuideTapIndicator />
        {checked ? <Image systemName="checkmark" size={15} color={guideColors.blue} /> : null}
        {chevron ? (
          <Image systemName="chevron.right" size={11} modifiers={[foregroundStyle('secondary')]} />
        ) : null}
      </HStack>
    </ZStack>
  );
}
