/**
 * Message Toast Component
 * 自动关闭的条幅提示组件
 */

import React, { useRef } from 'react';
import { Text, StyleSheet, Animated } from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';

export type MessageType = 'success' | 'error' | 'info';

interface Message {
  text: string;
  type: MessageType;
}

interface MessageToastProps {
  message: Message | null;
  onMessageShown: () => void;
}

export function MessageToast({ message, onMessageShown }: MessageToastProps) {
  const { theme } = useTheme();
  const fadeAnim = useRef(new Animated.Value(0)).current;

  React.useEffect(() => {
    if (message) {
      // 淡入动画
      Animated.sequence([
        Animated.timing(fadeAnim, {
          toValue: 1,
          duration: 300,
          useNativeDriver: true,
        }),
        Animated.delay(2500),
        Animated.timing(fadeAnim, {
          toValue: 0,
          duration: 300,
          useNativeDriver: true,
        }),
      ]).start(() => {
        onMessageShown();
      });
    }
  }, [message, fadeAnim, onMessageShown]);

  if (!message) {
    return null;
  }

  // M3 Snackbar 风:inverseSurface(成功 / info)/ errorContainer(错误)
  const bg =
    message.type === 'error'
      ? theme.colors.errorContainer
      : message.type === 'success'
        ? theme.colors.inverseSurface
        : theme.colors.inverseSurface;
  const fg =
    message.type === 'error' ? theme.colors.onErrorContainer : theme.colors.inverseOnSurface;

  return (
    <Animated.View
      style={[styles.messageContainer, { backgroundColor: bg }, { opacity: fadeAnim }]}
    >
      <Text style={[styles.messageText, { color: fg }]}>{message.text}</Text>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  messageContainer: {
    paddingVertical: spacing.md,
    paddingHorizontal: spacing.base,
    marginHorizontal: spacing.base,
    marginBottom: spacing.sm,
    borderRadius: radius.md,
    borderCurve: 'continuous',
    alignItems: 'center',
    justifyContent: 'center',
    ...elevation.md,
  },
  messageText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '500',
  },
});
