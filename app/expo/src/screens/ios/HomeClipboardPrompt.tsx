import React from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from 'react-native';
import Animated, { FadeInDown, FadeOutDown } from 'react-native-reanimated';
import * as Haptics from 'expo-haptics';
import { ClipboardPaste, Image as ImageIcon, Layers, X } from 'lucide-react-native';
import { GlassContainer } from '@/components/ui';
import type { ClipboardChangedEvent } from '@/engine/types';
import { iosAccent, iosColors, iosOnAccent } from '@/theme/iosDesignTokens';
import type { Theme } from '@/theme';

const PROMPT_ENTERING = FadeInDown.springify().damping(28).stiffness(360);
const PROMPT_EXITING = FadeOutDown.duration(160);

export interface HomeClipboardPromptProps {
  hint: ClipboardChangedEvent;
  bottomOffset: number;
  reading: boolean;
  label: string;
  subtitle: string;
  dismissLabel: string;
  theme: Theme;
  onRead: () => void;
  onDismiss: () => void;
}

/**
 * 方案 B：悬浮于底部 iOS 26 Liquid Glass 标签栏正上方的液态玻璃胶囊。
 * - 不挤占顶部大标题与网格高度（列表零跳动），随滚动常驻单手拇指热区；
 * - 利用原生无弹窗探测信号（hasStrings / hasImages）切换图标与文案；
 * - 右侧提供忽略按钮，方便复制密码等敏感内容时直接关闭而不读取。
 */
export function HomeClipboardPrompt({
  hint,
  bottomOffset,
  reading,
  label,
  subtitle,
  dismissLabel,
  theme,
  onRead,
  onDismiss,
}: HomeClipboardPromptProps) {
  const badgeBg = theme.isDark ? iosAccent.dark : iosAccent.light;
  const badgeFg = theme.isDark ? iosOnAccent.dark : iosOnAccent.light;
  const KindIcon = hint.hasImages && !hint.hasStrings
    ? ImageIcon
    : hint.hasImages && hint.hasStrings
      ? Layers
      : ClipboardPaste;

  const handleReadPress = () => {
    if (reading) return;
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {});
    onRead();
  };

  const handleDismissPress = () => {
    void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {});
    onDismiss();
  };

  return (
    <Animated.View
      pointerEvents="box-none"
      entering={PROMPT_ENTERING}
      exiting={PROMPT_EXITING}
      style={[styles.anchor, { bottom: bottomOffset }]}
    >
      <View style={styles.shadowWrap}>
        <GlassContainer shape="capsule" interactive style={styles.pill}>
          <Pressable
            testID="home-read-detected-clipboard"
            accessibilityRole="button"
            accessibilityLabel={label}
            accessibilityHint={subtitle}
            disabled={reading}
            onPress={handleReadPress}
            style={({ pressed }) => [styles.mainButton, pressed && styles.pressed]}
          >
            <View style={[styles.badge, { backgroundColor: badgeBg }]}>
              {reading ? (
                <ActivityIndicator size="small" color={badgeFg} />
              ) : (
                <KindIcon size={16} strokeWidth={2.1} color={badgeFg} />
              )}
            </View>
            <View style={styles.textWrap}>
              <Text
                numberOfLines={1}
                style={[styles.label, { color: iosColors?.label ?? theme.colors.textPrimary }]}
              >
                {label}
              </Text>
              <Text
                numberOfLines={1}
                style={[
                  styles.subtitle,
                  { color: iosColors?.secondaryLabel ?? theme.colors.textSecondary },
                ]}
              >
                {subtitle}
              </Text>
            </View>
          </Pressable>

          <View
            style={[
              styles.divider,
              { backgroundColor: iosColors?.separator ?? theme.colors.border },
            ]}
          />

          <Pressable
            testID="home-dismiss-detected-clipboard"
            accessibilityRole="button"
            accessibilityLabel={dismissLabel}
            hitSlop={6}
            onPress={handleDismissPress}
            style={({ pressed }) => [styles.dismissButton, pressed && styles.pressed]}
          >
            <X size={16} strokeWidth={2.2} color={theme.colors.textSecondary} />
          </Pressable>
        </GlassContainer>
      </View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  anchor: {
    position: 'absolute',
    left: 16,
    right: 16,
    alignItems: 'center',
  },
  shadowWrap: {
    maxWidth: '100%',
    borderRadius: 9999,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 8 },
    shadowOpacity: 0.14,
    shadowRadius: 18,
  },
  pill: {
    flexDirection: 'row',
    alignItems: 'center',
    height: 48,
    paddingLeft: 6,
    paddingRight: 6,
    maxWidth: '100%',
  },
  mainButton: {
    flexDirection: 'row',
    alignItems: 'center',
    flexShrink: 1,
    minHeight: 44,
    paddingLeft: 2,
    paddingRight: 10,
    gap: 9,
  },
  badge: {
    width: 32,
    height: 32,
    borderRadius: 16,
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
  textWrap: {
    flexShrink: 1,
    justifyContent: 'center',
  },
  label: {
    fontSize: 14,
    fontWeight: '600',
    lineHeight: 17,
    letterSpacing: -0.15,
  },
  subtitle: {
    fontSize: 11,
    fontWeight: '500',
    lineHeight: 14,
  },
  divider: {
    width: StyleSheet.hairlineWidth,
    height: 20,
    marginHorizontal: 2,
  },
  dismissButton: {
    width: 36,
    height: 36,
    borderRadius: 18,
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
  pressed: {
    opacity: 0.65,
  },
});
