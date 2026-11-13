import React from 'react';
import { HStack, Text as SwiftUIText, Spacer } from '@expo/ui/swift-ui';
import { font, frame, padding } from '@expo/ui/swift-ui/modifiers';

export interface SheetHeaderProps {
  title: string;
  left?: React.ReactNode;
  right?: React.ReactNode;
  leftSlots?: [React.ReactNode?, React.ReactNode?];
  rightSlots?: [React.ReactNode?, React.ReactNode?];
  compactSides?: boolean;
}

const HEADER_BUTTON_SLOT_COUNT = 2;
const HEADER_BUTTON_SLOT_SIZE = 44;
const HEADER_SIDE_MIN_WIDTH = HEADER_BUTTON_SLOT_COUNT * HEADER_BUTTON_SLOT_SIZE;

function orderHeaderButtonSlots(
  slots: [React.ReactNode?, React.ReactNode?],
  fillFrom: 'leading' | 'trailing'
): [React.ReactNode?, React.ReactNode?] {
  if (fillFrom === 'trailing') {
    return [slots[1], slots[0]];
  }
  return slots;
}

function renderHeaderButtonSlots(
  slots: [React.ReactNode?, React.ReactNode?],
  fillFrom: 'leading' | 'trailing'
) {
  const orderedSlots = orderHeaderButtonSlots(slots, fillFrom);

  return Array.from({ length: HEADER_BUTTON_SLOT_COUNT }, (_, index) => (
    <HStack
      key={`sheet-header-slot-${index}`}
      alignment="center"
      modifiers={[frame({ width: HEADER_BUTTON_SLOT_SIZE, height: HEADER_BUTTON_SLOT_SIZE })]}
    >
      {orderedSlots[index] ?? <Spacer />}
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
  const sideMinWidth = compactSides ? HEADER_BUTTON_SLOT_SIZE : HEADER_SIDE_MIN_WIDTH;

  return (
    <HStack
      alignment="center"
      modifiers={[frame({ maxWidth: Infinity }), padding({ horizontal: 20, top: 16, bottom: 12 })]}
    >
      {leftSlots ? (
        <HStack alignment="center">{renderHeaderButtonSlots(leftSlots, 'leading')}</HStack>
      ) : (
        renderAdaptiveHeaderSide(left, 'leading', sideMinWidth)
      )}
      <Spacer />
      <SwiftUIText modifiers={[font({ weight: 'bold', size: 17 })]}>{title}</SwiftUIText>
      <Spacer />
      {rightSlots ? (
        <HStack alignment="center">{renderHeaderButtonSlots(rightSlots, 'trailing')}</HStack>
      ) : (
        renderAdaptiveHeaderSide(right, 'trailing', sideMinWidth)
      )}
    </HStack>
  );
}
