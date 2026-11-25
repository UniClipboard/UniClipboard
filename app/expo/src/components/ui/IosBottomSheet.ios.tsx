import React from 'react';
import { StyleSheet } from 'react-native';
import { BottomSheet, Group, Host } from '@expo/ui/swift-ui';
import {
  interactiveDismissDisabled as interactiveDismissDisabledModifier,
  presentationBackground,
  presentationDetents,
  presentationDragIndicator,
  presentationSizing,
  tint,
  type ModifierConfig,
  type PresentationDetent,
  type PresentationSizingType,
  type ShapeStyle,
} from '@expo/ui/swift-ui/modifiers';

import { iosAccentColor, iosColors } from '@/theme/iosDesignTokens';
import { IosPageChromeProvider, type IosPageChrome } from './IosSheetPage';

const SHEET_CHROME: IosPageChrome = { kind: 'sheet' };
const DEFAULT_DETENTS: PresentationDetent[] = ['large'];

export interface IosBottomSheetProps {
  visible: boolean;
  onDismiss: () => void;
  onDidDismiss?: () => void;
  /** Snap heights when fitToContents is false. Defaults to ['large']. */
  detents?: PresentationDetent[];
  /** Automatically size the sheet height to fit its content. */
  fitToContents?: boolean;
  /** Drag indicator visibility. Defaults to 'visible'. */
  dragIndicator?: 'automatic' | 'visible' | 'hidden';
  /** Prevents interactive swipe-down dismissal (e.g. while an async save is running). */
  interactiveDismissDisabled?: boolean;
  /** Background fill painted across the full sheet chrome including safe area. */
  backgroundColor?: ShapeStyle;
  /** iPad presentation sizing hint. */
  sizing?: PresentationSizingType;
  /** Wrap in a zero-footprint SwiftUI <Host> when rendered from a React Native screen. */
  standalone?: boolean;
  children: React.ReactNode;
}

/**
 * Canonical native SwiftUI sheet presentation for iOS.
 *
 * Inside SwiftUI screens (`SettingsScreen.ios`, `DevicesScreen.ios`), mount as a
 * sibling of `NavigationStack` inside the screen's stable `<Host>` (`standalone={false}`).
 * Inside React Native screens (`HomeOverlays`, `AppNavigator`), pass `standalone`.
 */
export function IosBottomSheet({
  visible,
  onDismiss,
  onDidDismiss,
  detents = DEFAULT_DETENTS,
  fitToContents = false,
  dragIndicator = 'visible',
  interactiveDismissDisabled = false,
  backgroundColor = iosColors?.systemGroupedBackground ?? '#F2F2F7',
  sizing,
  standalone = false,
  children,
}: IosBottomSheetProps) {
  const effectiveSizing = sizing ?? (fitToContents ? 'fitted' : 'form');
  const groupModifiers: ModifierConfig[] = [
    ...(fitToContents ? [] : [presentationDetents(detents)]),
    presentationDragIndicator(dragIndicator),
    presentationSizing(effectiveSizing),
    presentationBackground(backgroundColor),
    ...(interactiveDismissDisabled ? [interactiveDismissDisabledModifier(true)] : []),
    ...(iosAccentColor ? [tint(iosAccentColor)] : []),
  ];

  const sheet = (
    <BottomSheet
      isPresented={visible}
      fitToContents={fitToContents}
      onIsPresentedChange={(presented) => {
        if (!presented) onDismiss();
      }}
      onDismiss={onDidDismiss}
    >
      <Group modifiers={groupModifiers}>
        <IosPageChromeProvider value={SHEET_CHROME}>{children}</IosPageChromeProvider>
      </Group>
    </BottomSheet>
  );

  if (!standalone) return sheet;

  return (
    <Host style={styles.standaloneHost} pointerEvents="none">
      {sheet}
    </Host>
  );
}

const styles = StyleSheet.create({
  standaloneHost: {
    position: 'absolute',
    width: 0,
    height: 0,
  },
});
