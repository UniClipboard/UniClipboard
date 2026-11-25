import React, { createContext, useContext } from 'react';
import { Form, Toolbar, VStack } from '@expo/ui/swift-ui';
import {
  background,
  frame,
  listStyle,
  navigationTitle,
  scrollContentBackground,
  scrollDismissesKeyboard,
  tint,
  type ModifierConfig,
} from '@expo/ui/swift-ui/modifiers';

import { iosAccentColor, iosColors } from '@/theme/iosDesignTokens';
import { SheetHeader, type SheetHeaderProps } from './SheetHeader';

export interface IosSheetPageProps extends SheetHeaderProps {
  children: React.ReactNode;
  spacing?: number;
  modifiers?: ModifierConfig[];
}

export interface IosSheetFormProps {
  children: React.ReactNode;
  modifiers?: ModifierConfig[];
}

/**
 * Chrome context for SwiftUI pages on iOS:
 * - sheet: presented inside a bottom sheet with a compact `SheetHeader`
 * - navigation: pushed inside a SwiftUI `NavigationStack`, using the native navigation bar
 */
export interface IosPageChrome {
  kind: 'sheet' | 'navigation';
}

const IosPageChromeContext = createContext<IosPageChrome>({ kind: 'sheet' });

export function IosPageChromeProvider({
  value,
  children,
}: {
  value: IosPageChrome;
  children: React.ReactNode;
}) {
  return <IosPageChromeContext.Provider value={value}>{children}</IosPageChromeContext.Provider>;
}

export function useIosPageChrome(): IosPageChrome {
  return useContext(IosPageChromeContext);
}

const sheetPageBackgroundColor = iosColors?.systemGroupedBackground ?? '#F2F2F7';
const sheetPageBaseModifiers = [
  frame({ maxWidth: Infinity, maxHeight: Infinity, alignment: 'top' }),
  background(sheetPageBackgroundColor),
  ...(iosAccentColor ? [tint(iosAccentColor)] : []),
];
const sheetFormBaseModifiers = [
  listStyle('insetGrouped'),
  scrollContentBackground('hidden'),
  scrollDismissesKeyboard('interactively'),
  background(sheetPageBackgroundColor),
];

export function IosSheetPage({
  title,
  left,
  right,
  leftSlots,
  rightSlots,
  compactSides,
  children,
  spacing = 0,
  modifiers = [],
}: IosSheetPageProps) {
  const chrome = useIosPageChrome();
  if (chrome.kind === 'navigation') {
    const trailing = rightSlots ? rightSlots.filter(Boolean) : right ? [right] : [];
    const page = (
      <VStack
        spacing={spacing}
        modifiers={[
          ...sheetPageBaseModifiers,
          ...modifiers,
          ...(trailing.length ? [] : [navigationTitle(title)]),
        ]}
      >
        {children}
      </VStack>
    );
    if (!trailing.length) return page;
    return (
      <Toolbar modifiers={[navigationTitle(title)]}>
        {page}
        <Toolbar.Content>{trailing}</Toolbar.Content>
      </Toolbar>
    );
  }
  return (
    <VStack spacing={spacing} modifiers={[...sheetPageBaseModifiers, ...modifiers]}>
      <SheetHeader
        title={title}
        left={left}
        right={right}
        leftSlots={leftSlots}
        rightSlots={rightSlots}
        compactSides={compactSides}
      />
      {children}
    </VStack>
  );
}

export function IosSheetForm({ children, modifiers = [] }: IosSheetFormProps) {
  return <Form modifiers={[...sheetFormBaseModifiers, ...modifiers]}>{children}</Form>;
}
