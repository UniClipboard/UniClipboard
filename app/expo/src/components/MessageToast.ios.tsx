/**
 * iOS 消息提示,两种形态:
 * - 宿主传 bottomOffset(首页):底部 Liquid Glass 胶囊 + 状态图标,从下方淡入上浮,停在
 *   标签栏 / 详情工具栏之上;
 * - 其他场景:顶部深色胶囊,从屏幕顶端外弹簧滑入,设置页等由宿主传 topOffset。
 */
import React, { useRef } from 'react';
import { Text, StyleSheet, Animated, PlatformColor, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { CircleAlert, CircleCheck, Info } from 'lucide-react-native';
import { GlassContainer } from '@/components/ui';
import { duration, easing, overlayMotion } from '@/theme/motion';
import type { MessageType, MessageToastProps } from './MessageToast.types';

export type { MessageType } from './MessageToast.types';

// 首页顶栏:paddingTop insets.top+4 → 行高 52 → paddingBottom 4,再留 8 间距,
// 避免遮挡「选择」按钮那一排
const HOME_TOP_BAR_CLEARANCE = 68;

// 错误信息需要更长阅读时间
const HOLD_MS: Record<MessageType, number> = {
  success: 1600,
  error: 2600,
  info: 2000,
};

export function MessageToast({
  message,
  onMessageShown,
  topOffset,
  bottomOffset,
}: MessageToastProps) {
  const insets = useSafeAreaInsets();
  const progress = useRef(new Animated.Value(0)).current;

  React.useEffect(() => {
    if (!message) return;
    progress.setValue(0);
    Animated.sequence([
      Animated.spring(progress, {
        toValue: 1,
        ...overlayMotion.enterSpring,
        useNativeDriver: true,
      }),
      Animated.delay(HOLD_MS[message.type]),
      Animated.timing(progress, {
        toValue: 0,
        duration: duration.base,
        easing: easing.accelerate,
        useNativeDriver: true,
      }),
    ]).start(({ finished }) => {
      // 被新消息打断时不清空,让新一轮动画接管
      if (finished) onMessageShown();
    });
  }, [message, progress, onMessageShown]);

  if (!message) {
    return null;
  }

  if (bottomOffset != null) {
    const Icon = STATUS_ICON[message.type];
    return (
      <Animated.View
        pointerEvents="none"
        accessibilityRole="alert"
        style={[
          styles.bottomSlot,
          {
            bottom: bottomOffset,
            opacity: progress.interpolate({ inputRange: [0, 0.4, 1], outputRange: [0, 1, 1] }),
            transform: [
              {
                translateY: progress.interpolate({ inputRange: [0, 1], outputRange: [16, 0] }),
              },
            ],
          },
        ]}
      >
        <GlassContainer shape="capsule" style={styles.glassPill}>
          <View style={styles.glassRow}>
            <Icon size={19} strokeWidth={2.2} color={STATUS_COLOR[message.type]} />
            <Text style={[styles.glassText, { color: PlatformColor('label') }]} numberOfLines={2}>
              {message.text}
            </Text>
          </View>
        </GlassContainer>
      </Animated.View>
    );
  }

  const top = topOffset ?? insets.top + HOME_TOP_BAR_CLEARANCE;
  // 完全收起时整个胶囊(含两行文本的情况)都在屏幕上边缘之外
  const hiddenOffset = -(top + 96);

  return (
    <Animated.View
      pointerEvents="none"
      accessibilityRole="alert"
      style={[
        styles.pill,
        {
          top,
          opacity: progress.interpolate({ inputRange: [0, 0.3, 1], outputRange: [0, 1, 1] }),
          transform: [
            {
              translateY: progress.interpolate({
                inputRange: [0, 1],
                outputRange: [hiddenOffset, 0],
              }),
            },
          ],
        },
      ]}
    >
      <Text style={styles.text} numberOfLines={2}>
        {message.text}
      </Text>
    </Animated.View>
  );
}

const STATUS_ICON = { success: CircleCheck, error: CircleAlert, info: Info } as const;
const STATUS_COLOR = {
  success: PlatformColor('systemGreen'),
  error: PlatformColor('systemRed'),
  info: PlatformColor('secondaryLabel'),
} as const;

const styles = StyleSheet.create({
  bottomSlot: { position: 'absolute', left: 24, right: 24, alignItems: 'center' },
  glassPill: { minHeight: 44, paddingLeft: 14, paddingRight: 18, justifyContent: 'center' },
  glassRow: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 10 },
  glassText: { flexShrink: 1, fontSize: 15, fontWeight: '600', lineHeight: 20 },
  pill: {
    position: 'absolute',
    alignSelf: 'center',
    maxWidth: '86%',
    paddingVertical: 12,
    paddingHorizontal: 22,
    borderRadius: 22,
    borderCurve: 'continuous',
    backgroundColor: 'rgba(44, 44, 46, 0.96)',
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 4 },
    shadowOpacity: 0.18,
    shadowRadius: 12,
  },
  text: {
    fontSize: 15,
    fontWeight: '600',
    lineHeight: 20,
    textAlign: 'center',
    color: '#FFFFFF',
  },
});
