import React from 'react';
import { Button as SwiftUIButton, HStack, Image, Spacer, Text as SwiftUIText } from '@expo/ui/swift-ui';
import {
  accessibilityAddTraits,
  accessibilityLabel as accessibilityLabelModifier,
  buttonStyle,
  disabled as disabledModifier,
  font,
  frame,
  glassEffect,
  layoutPriority,
  lineLimit,
  minimumScaleFactor,
  multilineTextAlignment,
  padding,
  truncationMode,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import { iosColors } from '@/theme/iosDesignTokens';

export interface SheetHeaderProps {
  title: string;
  left?: React.ReactNode;
  right?: React.ReactNode;
  leftSlots?: [React.ReactNode?, React.ReactNode?];
  rightSlots?: [React.ReactNode?, React.ReactNode?];
  compactSides?: boolean;
}

export interface SheetCircleButtonProps {
  testID?: string;
  systemName: SFSymbol;
  onPress: () => void;
  accessibilityLabel?: string;
  disabled?: boolean;
}

const HEADER_BUTTON_SLOT_SIZE = 44;
const HEADER_CIRCLE_BUTTON_SIZE = 34;
const HEADER_ICON_FALLBACK_COLOR = '#8E8E93';

/** Glass circular header button (close, back, add, ...) for iOS sheet headers. */
export function SheetCircleButton({
  testID,
  systemName,
  onPress,
  accessibilityLabel,
  disabled = false,
}: SheetCircleButtonProps) {
  return (
    <SwiftUIButton
      testID={testID}
      onPress={onPress}
      modifiers={[
        buttonStyle('plain'),
        disabledModifier(disabled),
        frame({ width: HEADER_CIRCLE_BUTTON_SIZE, height: HEADER_CIRCLE_BUTTON_SIZE }),
        glassEffect({ glass: { variant: 'regular', interactive: true }, shape: 'circle' }),
        ...(accessibilityLabel ? [accessibilityLabelModifier(accessibilityLabel)] : []),
      ]}
    >
      <Image
        systemName={systemName}
        size={15}
        color={iosColors?.secondaryLabel ?? HEADER_ICON_FALLBACK_COLOR}
        modifiers={[
          font({ size: 15, weight: 'semibold' }),
          frame({ width: HEADER_CIRCLE_BUTTON_SIZE, height: HEADER_CIRCLE_BUTTON_SIZE }),
        ]}
      />
    </SwiftUIButton>
  );
}

function slotCount(slots?: [React.ReactNode?, React.ReactNode?]): number {
  if (!slots) return 0;
  return slots[1] ? 2 : slots[0] ? 1 : 0;
}

function orderHeaderButtonSlots(
  slots: [React.ReactNode?, React.ReactNode?],
  count: number,
  fillFrom: 'leading' | 'trailing'
): React.ReactNode[] {
  const sliced = slots.slice(0, count);
  while (sliced.length < count) sliced.push(undefined);
  return fillFrom === 'trailing' ? sliced.reverse() : sliced;
}

function renderHeaderButtonSlots(
  slots: [React.ReactNode?, React.ReactNode?],
  count: number,
  fillFrom: 'leading' | 'trailing'
) {
  const orderedSlots = orderHeaderButtonSlots(slots, count, fillFrom);

  return orderedSlots.map((slot, index) => (
    <HStack
      key={`sheet-header-slot-${index}`}
      alignment="center"
      modifiers={[frame({ width: HEADER_BUTTON_SLOT_SIZE, height: HEADER_BUTTON_SLOT_SIZE })]}
    >
      {slot ?? <Spacer />}
    </HStack>
  ));
}

function renderAdaptiveHeaderSide(
  content: React.ReactNode,
  fillFrom: 'leading' | 'trailing',
  minWidth: number
) {
  return (
    <HStack
      alignment="center"
      modifiers={[
        frame({
          minWidth,
          minHeight: HEADER_BUTTON_SLOT_SIZE,
          alignment: fillFrom === 'leading' ? 'leading' : 'trailing',
        }),
      ]}
    >
      {content}
    </HStack>
  );
}

export function SheetHeader({
  title,
  left,
  right,
  leftSlots,
  rightSlots,
  compactSides = false,
}: SheetHeaderProps) {
  const neededSlots = compactSides ? 1 : Math.max(1, slotCount(leftSlots), slotCount(rightSlots));
  const sideMinWidth = neededSlots * HEADER_BUTTON_SLOT_SIZE;

  return (
    <HStack
      alignment="center"
      spacing={8}
      modifiers={[frame({ maxWidth: Infinity }), padding({ horizontal: 16, top: 18, bottom: 10 })]}
    >
      {leftSlots ? (
        <HStack alignment="center" spacing={0}>
          {renderHeaderButtonSlots(leftSlots, neededSlots, 'leading')}
        </HStack>
      ) : (
        renderAdaptiveHeaderSide(left, 'leading', sideMinWidth)
      )}
      <Spacer />
      <SwiftUIText
        modifiers={[
          font({ textStyle: 'headline', weight: 'semibold' }),
          lineLimit(1),
          minimumScaleFactor(0.8),
          truncationMode('tail'),
          multilineTextAlignment('center'),
          layoutPriority(1),
          accessibilityAddTraits(['isHeader']),
        ]}
      >
        {title}
      </SwiftUIText>
      <Spacer />
      {rightSlots ? (
        <HStack alignment="center" spacing={0}>
          {renderHeaderButtonSlots(rightSlots, neededSlots, 'trailing')}
        </HStack>
      ) : (
        renderAdaptiveHeaderSide(right, 'trailing', sideMinWidth)
      )}
    </HStack>
  );
}
