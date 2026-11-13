/**
 * Android 消息提示 — Material 3 Snackbar
 *
 * 底部锚定、反色表面(inverseSurface)、可带一个文字操作(inversePrimary,如「撤销」「更新」)。
 * 自下方淡入上移,停留后淡出。首页由宿主传 bottomOffset 抬到 FAB / 多选底栏之上;
 * 其它场景默认贴导航栏上方 16dp。
 * 带操作时停留更久;开启 TalkBack 等读屏时再延长,保证读屏用户来得及聚焦操作按钮。
 */
import React, { useEffect, useRef } from 'react';
import { AccessibilityInfo, Animated, Pressable, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTheme } from '@/hooks/useTheme';
import { duration, easing } from '@/theme/motion';
import { m3Type } from '@/theme/m3Typography';
import type { Message, MessageToastProps, MessageType } from './MessageToast.types';

export type { MessageType } from './MessageToast.types';

// M3:短消息 4s;错误需要更长阅读时间;带操作的消息给足点按时间
const HOLD_MS: Record<MessageType, number> = {
  success: 2500,
  info: 3000,
  error: 5000,
};
const ACTION_HOLD_MS = 6000;
const SCREEN_READER_HOLD_MS = 10000;

function getSnackbarHoldMs(message: Message, screenReaderEnabled: boolean): number {
  const base = message.action ? ACTION_HOLD_MS : HOLD_MS[message.type];
  return screenReaderEnabled ? Math.max(base, SCREEN_READER_HOLD_MS) : base;
}

export function MessageToast({ message, onMessageShown, bottomOffset }: MessageToastProps) {
  const insets = useSafeAreaInsets();
  const { theme } = useTheme();
  const progress = useRef(new Animated.Value(0)).current;
  const animationRef = useRef<Animated.CompositeAnimation | null>(null);
  // 读屏状态只在排定下一条消息的停留时长时读取,不参与渲染 → 用 ref,不触发重渲
  const screenReaderEnabledRef = useRef(false);

  useEffect(() => {
    let active = true;
    const update = (enabled: boolean) => {
      if (active) screenReaderEnabledRef.current = enabled;
    };
    AccessibilityInfo.isScreenReaderEnabled()
      .then(update)
      .catch(() => {});
    const sub = AccessibilityInfo.addEventListener('screenReaderChanged', update);
    return () => {
      active = false;
      sub.remove();
    };
  }, []);

  useEffect(() => {
    if (!message) return;
    progress.setValue(0);
    const animation = Animated.sequence([
      Animated.timing(progress, {
        toValue: 1,
        duration: duration.base,
        easing: easing.decelerate,
        useNativeDriver: true,
      }),
      Animated.delay(getSnackbarHoldMs(message, screenReaderEnabledRef.current)),
      Animated.timing(progress, {
        toValue: 0,
        duration: duration.fast,
        easing: easing.accelerate,
        useNativeDriver: true,
      }),
    ]);
    animationRef.current = animation;
    animation.start(({ finished }) => {
      // 被新消息或操作按钮打断时不在这里收尾
      if (!finished) return;
      message.onTimeout?.();
      onMessageShown();
    });
    return () => animation.stop();
  }, [message, progress, onMessageShown]);

  if (!message) {
    return null;
  }

  const handleAction = () => {
    animationRef.current?.stop();
    message.action?.onPress();
    Animated.timing(progress, {
      toValue: 0,
      duration: duration.fast,
      easing: easing.accelerate,
      useNativeDriver: true,
    }).start(() => onMessageShown());
  };

  const bottom = bottomOffset ?? insets.bottom + 16;

  return (
    <View pointerEvents="box-none" style={[styles.host, { bottom }]}>
      <Animated.View
        testID="snackbar"
        accessibilityLiveRegion="polite"
        pointerEvents={message.action ? 'auto' : 'none'}
        style={[
          styles.snackbar,
          {
            backgroundColor: theme.colors.inverseSurface,
            opacity: progress,
            transform: [
              {
                translateY: progress.interpolate({ inputRange: [0, 1], outputRange: [12, 0] }),
              },
            ],
          },
        ]}
      >
        <Text style={[styles.text, { color: theme.colors.inverseOnSurface }]} numberOfLines={2}>
          {message.text}
        </Text>
        {message.action ? (
          <Pressable
            testID="snackbar-action"
            onPress={handleAction}
            accessibilityRole="button"
            accessibilityLabel={message.action.label}
            android_ripple={{ color: theme.colors.fillSecondary as string }}
            style={styles.action}
          >
            <Text style={[styles.actionLabel, { color: theme.colors.inverseAccent }]}>
              {message.action.label}
            </Text>
          </Pressable>
        ) : null}
      </Animated.View>
    </View>
  );
}

const styles = StyleSheet.create({
  host: {
    position: 'absolute',
    left: 16,
    right: 16,
    alignItems: 'center',
  },
  snackbar: {
    width: '100%',
    maxWidth: 600,
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    paddingLeft: 16,
    paddingRight: 8,
    borderRadius: 4,
    elevation: 6,
  },
  text: {
    ...m3Type.bodyMedium,
    flex: 1,
    paddingVertical: 14,
    paddingRight: 8,
  },
  action: {
    minHeight: 48,
    paddingHorizontal: 12,
    justifyContent: 'center',
    borderRadius: 4,
    overflow: 'hidden',
  },
  actionLabel: {
    ...m3Type.labelLarge,
  },
});
