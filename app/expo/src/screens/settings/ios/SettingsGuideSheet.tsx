import React, { useEffect, useState } from 'react';
import { DynamicColorIOS } from 'react-native';
import {
  Button as SwiftUIButton,
  Capsule,
  Circle,
  HStack,
  Image,
  RoundedRectangle,
  ScrollView,
  Spacer,
  TabView,
  Text as SwiftUIText,
  VStack,
  ZStack,
} from '@expo/ui/swift-ui';
import {
  Animation,
  accessibilityHidden,
  animation,
  background,
  controlSize,
  font,
  foregroundStyle,
  frame,
  padding,
  shapes,
  strokeBorder,
  tabViewStyle,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import { IosBottomSheet, SheetCircleButton, SheetHeader } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import { iosColors } from '@/theme/iosDesignTokens';
import { settingsTileColors } from './common';

/** Colors shared by the drawings in the illustrated settings guides. */
export const guideColors = {
  sheetBackground: iosColors?.systemBackground ?? '#FFFFFF',
  tipBackground: iosColors?.secondarySystemBackground ?? '#F2F2F7',
  canvas: iosColors?.systemGroupedBackground ?? '#F2F2F7',
  card: iosColors?.secondarySystemGroupedBackground ?? '#FFFFFF',
  border: iosColors?.separator ?? '#C6C6C8',
  placeholder: iosColors?.tertiarySystemFill ?? '#E5E5EA',
  neutralTile: DynamicColorIOS({ light: '#C7C7CC', dark: '#48484A' }),
  progressTrack: DynamicColorIOS({ light: '#E5E5EA', dark: '#3A3A3C' }),
  highlightFill: DynamicColorIOS({ light: '#E8F1FC', dark: '#0B2A4A' }),
  warningBackground: DynamicColorIOS({ light: '#FFF4E5', dark: '#3A2A12' }),
  blue: settingsTileColors.blue,
} as const;

export type GuideColor = string | ReturnType<typeof DynamicColorIOS>;

export const GUIDE_CANVAS_HEIGHT = 300;

export interface SettingsGuideSheetProps<Step extends string> {
  visible: boolean;
  onClose: () => void;
  testIDPrefix: string;
  title: string;
  steps: readonly Step[];
  renderStep: (step: Step) => React.ReactNode;
  primaryAction: { label: string; systemImage: SFSymbol; onPress: () => void };
  labels: {
    progress: (current: number, total: number) => string;
    back: string;
    close: string;
    next: string;
    done: string;
  };
}

/**
 * Large-detent sheet that walks through a task in the iOS system UI one
 * swipeable page at a time (aligned with v1 SettingsGuideSheet.tsx).
 */
export function SettingsGuideSheet<Step extends string>({
  visible,
  onClose,
  testIDPrefix,
  title,
  steps,
  renderStep,
  primaryAction,
  labels,
}: SettingsGuideSheetProps<Step>) {
  const [stepIndex, setStepIndex] = useState(0);
  const isLastStep = stepIndex === steps.length - 1;

  useEffect(() => {
    if (visible) setStepIndex(0);
  }, [visible]);

  return (
    <IosBottomSheet
      visible={visible}
      onDismiss={onClose}
      detents={['large']}
      backgroundColor={guideColors.sheetBackground}
    >
      <VStack
        spacing={0}
        modifiers={[
          frame({ maxWidth: Infinity, maxHeight: Infinity }),
          background(guideColors.sheetBackground),
        ]}
      >
        <SheetHeader
          title={title}
          leftSlots={
            stepIndex > 0
              ? [
                  <SheetCircleButton
                    key="back"
                    testID={`${testIDPrefix}-back`}
                    systemName="chevron.backward"
                    accessibilityLabel={labels.back}
                    onPress={() => setStepIndex((current) => Math.max(0, current - 1))}
                  />,
                ]
              : undefined
          }
          rightSlots={[
            <SheetCircleButton
              key="close"
              testID={`${testIDPrefix}-close`}
              systemName="xmark"
              accessibilityLabel={labels.close}
              onPress={onClose}
            />,
          ]}
        />
        <GuideProgress
          stepIndex={stepIndex}
          stepCount={steps.length}
          label={labels.progress(stepIndex + 1, steps.length)}
        />
        <TabView
          selection={String(stepIndex)}
          onSelectionChange={(value) => setStepIndex(Number(value))}
          modifiers={[
            tabViewStyle({ type: 'page', indexDisplayMode: 'never' }),
            frame({ maxWidth: Infinity, maxHeight: Infinity }),
            animation(Animation.easeInOut({ duration: 0.3 }), stepIndex),
          ]}
        >
          {steps.map((step, index) => (
            <TabView.Tab key={step} value={String(index)}>
              {renderStep(step)}
            </TabView.Tab>
          ))}
        </TabView>
        <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 10, bottom: 16 })]}>
          <SwiftUIButton
            testID={`${testIDPrefix}-primary`}
            onPress={primaryAction.onPress}
            modifiers={[
              ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
              controlSize('large'),
            ]}
          >
            <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
              <Spacer />
              <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                {primaryAction.label}
              </SwiftUIText>
              <Image systemName={primaryAction.systemImage} size={14} />
              <Spacer />
            </HStack>
          </SwiftUIButton>
          <SwiftUIButton
            testID={`${testIDPrefix}-next`}
            onPress={
              isLastStep
                ? onClose
                : () => setStepIndex((current) => Math.min(steps.length - 1, current + 1))
            }
            modifiers={[
              ...iosSecondaryButtonModifiers({ fullWidth: true }),
              controlSize('large'),
            ]}
          >
            <HStack modifiers={[frame({ maxWidth: Infinity })]}>
              <Spacer />
              <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                {isLastStep ? labels.done : labels.next}
              </SwiftUIText>
              <Spacer />
            </HStack>
          </SwiftUIButton>
        </VStack>
      </VStack>
    </IosBottomSheet>
  );
}

function GuideProgress({
  stepIndex,
  stepCount,
  label,
}: {
  stepIndex: number;
  stepCount: number;
  label: string;
}) {
  return (
    <VStack
      alignment="leading"
      spacing={8}
      modifiers={[frame({ maxWidth: Infinity }), padding({ horizontal: 20, top: 4, bottom: 12 })]}
    >
      <SwiftUIText
        modifiers={[font({ size: 13, weight: 'semibold' }), foregroundStyle('secondary')]}
      >
        {label}
      </SwiftUIText>
      <HStack spacing={6} modifiers={[accessibilityHidden(true)]}>
        {Array.from({ length: stepCount }, (_, index) => (
          <Capsule
            key={index}
            modifiers={[
              foregroundStyle(index <= stepIndex ? guideColors.blue : guideColors.progressTrack),
              frame({ maxWidth: Infinity, height: 4 }),
              animation(Animation.easeInOut({ duration: 0.3 }), stepIndex),
            ]}
          />
        ))}
      </HStack>
    </VStack>
  );
}

/** One guide page: the drawing, then its title, description and an optional tip. */
export function GuideStepPage({
  illustration,
  title,
  description,
  tip,
}: {
  illustration: React.ReactNode;
  title: string;
  description: string;
  tip?: { tone: 'info' | 'warning'; text: string };
}) {
  return (
    <ScrollView modifiers={[frame({ maxWidth: Infinity, maxHeight: Infinity })]}>
      <VStack
        alignment="leading"
        spacing={16}
        modifiers={[
          frame({ maxWidth: Infinity, alignment: 'leading' }),
          padding({ horizontal: 20, bottom: 12 }),
        ]}
      >
        {illustration}
        <VStack alignment="leading" spacing={8}>
          <SwiftUIText modifiers={[font({ size: 24, weight: 'bold' })]}>{title}</SwiftUIText>
          <SwiftUIText modifiers={[font({ size: 15 }), foregroundStyle('secondary')]}>
            {description}
          </SwiftUIText>
        </VStack>
        {tip ? <GuideTip tone={tip.tone} text={tip.text} /> : null}
      </VStack>
    </ScrollView>
  );
}

function GuideTip({ tone, text }: { tone: 'info' | 'warning'; text: string }) {
  const warning = tone === 'warning';
  return (
    <HStack
      spacing={10}
      alignment="top"
      modifiers={[
        frame({ maxWidth: Infinity, alignment: 'leading' }),
        padding({ horizontal: 14, vertical: 12 }),
        background(
          warning ? guideColors.warningBackground : guideColors.tipBackground,
          shapes.roundedRectangle({ cornerRadius: 14 })
        ),
      ]}
    >
      <Image
        systemName={warning ? 'exclamationmark.triangle.fill' : 'info.circle'}
        size={15}
        color={warning ? settingsTileColors.orange : undefined}
        modifiers={warning ? [] : [foregroundStyle('secondary')]}
      />
      <SwiftUIText
        modifiers={[font({ size: 13 }), frame({ maxWidth: Infinity, alignment: 'leading' })]}
      >
        {text}
      </SwiftUIText>
    </HStack>
  );
}

/** Rounded-square app or settings icon inside a drawing. */
export function GuideTileIcon({
  icon,
  color,
  size = 26,
  cornerRadius = 6,
}: {
  icon?: SFSymbol;
  color: GuideColor;
  size?: number;
  cornerRadius?: number;
}) {
  return (
    <ZStack modifiers={[frame({ width: size, height: size })]}>
      <RoundedRectangle cornerRadius={cornerRadius} modifiers={[foregroundStyle(color)]} />
      {icon ? <Image systemName={icon} size={Math.round(size / 2)} color="white" /> : null}
    </ZStack>
  );
}

/** Touch marker showing where to tap. */
export function GuideTapIndicator() {
  return (
    <Circle
      modifiers={[
        foregroundStyle(`${guideColors.blue}38`),
        frame({ width: 24, height: 24 }),
        strokeBorder({
          content: guideColors.blue,
          style: { lineWidth: 2 },
          shape: 'circle',
        }),
      ]}
    />
  );
}
