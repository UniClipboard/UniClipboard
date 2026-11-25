import React from 'react';
import {
  KeyboardAvoidingView,
  ScrollView,
  StyleSheet,
  useWindowDimensions,
  View,
  type ColorValue,
} from 'react-native';
import { RNHostView } from '@expo/ui/swift-ui';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { iosColors } from '@/theme/iosDesignTokens';
import { IosBottomSheet } from './IosBottomSheet';

export interface AppBottomSheetProps {
  visible: boolean;
  onDismiss: () => void;
  children: React.ReactNode;
  containerColor?: ColorValue;
}

/**
 * Native iOS bottom sheet bridge for shared React Native content.
 *
 * Delegates presentation, drag-to-dismiss, drag indicator, detents, and safe-area
 * chrome to SwiftUI's native `BottomSheet` via `IosBottomSheet` + `RNHostView`.
 */
export function AppBottomSheet({
  visible,
  onDismiss,
  children,
  containerColor,
}: AppBottomSheetProps) {
  const { width } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const sheetBg = containerColor ?? iosColors?.systemGroupedBackground ?? '#F2F2F7';

  return (
    <IosBottomSheet
      standalone
      visible={visible}
      onDismiss={onDismiss}
      detents={['medium', 'large']}
      backgroundColor={sheetBg}
    >
      <RNHostView>
        <KeyboardAvoidingView style={[styles.container, { maxWidth: width }]} behavior="padding">
          <ScrollView
            bounces={false}
            keyboardShouldPersistTaps="handled"
            contentContainerStyle={[
              styles.content,
              { paddingBottom: Math.max(insets.bottom, 20) },
            ]}
          >
            <View>{children}</View>
          </ScrollView>
        </KeyboardAvoidingView>
      </RNHostView>
    </IosBottomSheet>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    width: '100%',
  },
  content: {
    paddingHorizontal: 16,
    paddingTop: 20,
  },
});
