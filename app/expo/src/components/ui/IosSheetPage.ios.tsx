import React, { createContext, useContext } from 'react';
import { Form, Toolbar, VStack } from '@expo/ui/swift-ui';
import {
  background,
  frame,
  listStyle,
  navigationTitle,
  padding,
  scrollContentBackground,
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

export interface IosSheetScaffoldProps {
  children: React.ReactNode;
  footer?: React.ReactNode;
  contentAlignment?: 'top' | 'center';
  modifiers?: ModifierConfig[];
}

/**
 * 页面所在的外壳:
 * - sheet:底部弹层内的自绘页头(标题 + 左右圆形按钮),子页由宿主自行做推入动画;
 * - navigation:SwiftUI NavigationStack 内(iOS 标签页),标题交给原生导航栏(根页为大标题),
 *   返回由导航栈提供,右侧按钮进工具栏;原生标签栏的底部空间由系统 safe area 提供。
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
  background(sheetPageBackgroundColor),
];

export function IosSheetPage({
  title,
  left,
  right,
  leftSlots,
  rightSlots,
  children,
  spacing,
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
      />
      {children}
    </VStack>
  );
}

export function IosSheetForm({ children, modifiers = [] }: IosSheetFormProps) {
  return <Form modifiers={[...sheetFormBaseModifiers, ...modifiers]}>{children}</Form>;
}

export function IosSheetScaffold({
  children,
  footer,
  contentAlignment = 'top',
  modifiers = [],
}: IosSheetScaffoldProps) {
  return (
    <VStack
      spacing={0}
      modifiers={[frame({ maxWidth: Infinity, maxHeight: Infinity }), ...modifiers]}
    >
      <VStack
        spacing={0}
        modifiers={[
          frame({
            maxWidth: Infinity,
            maxHeight: Infinity,
            alignment: contentAlignment,
          }),
        ]}
      >
        {children}
      </VStack>
      {footer ? (
        <VStack
          spacing={4}
          modifiers={[
            frame({ maxWidth: Infinity }),
            padding({ horizontal: 20, top: 10, bottom: 16 }),
          ]}
        >
          {footer}
        </VStack>
      ) : null}
    </VStack>
  );
}
