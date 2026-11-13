import React from 'react';
import { Pressable, StyleSheet, View, type ColorValue } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { M3_MIN_TOUCH_TARGET } from '@/theme/m3Typography';
import type { ColorScheme } from '@/theme/colors';

export interface M3IconButtonProps {
  icon: keyof typeof Ionicons.glyphMap;
  /** 必填:图标按钮没有可见文字,TalkBack 只能读这个 */
  accessibilityLabel: string;
  onPress: () => void;
  colors: ColorScheme;
  /** standard=透明底;tonal=secondaryContainer 底(M3 filled tonal icon button) */
  variant?: 'standard' | 'tonal';
  /** 可见容器尺寸:standard=40dp(Compose IconButton 默认);large=48dp,与同行 48dp 高的按钮对齐 */
  size?: 'standard' | 'large';
  iconColor?: ColorValue;
  disabled?: boolean;
  testID?: string;
}

/**
 * M3 图标按钮(Android)。触控区恒为 48dp,波纹为 40dp 圆形 state layer,
 * 与 Compose IconButton 的尺寸/反馈一致。首页顶栏、多选栏、全屏页标题栏共用。
 */
export function M3IconButton({
  icon,
  accessibilityLabel,
  onPress,
  colors,
  variant = 'standard',
  size = 'standard',
  iconColor,
  disabled = false,
  testID,
}: M3IconButtonProps) {
  const color = disabled ? colors.textDisabled : iconColor ?? colors.textSecondary;
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityState={{ disabled }}
      android_ripple={{
        color: colors.fillSecondary as string,
        borderless: true,
        radius: size === 'large' ? 24 : 20,
      }}
      style={styles.target}
    >
      <View
        style={[
          styles.container,
          size === 'large' && styles.containerLarge,
          variant === 'tonal' && { backgroundColor: colors.surfaceHighest },
        ]}
      >
        <Ionicons name={icon} size={24} color={color} />
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  target: {
    width: M3_MIN_TOUCH_TARGET,
    height: M3_MIN_TOUCH_TARGET,
    alignItems: 'center',
    justifyContent: 'center',
  },
  container: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
  },
  containerLarge: {
    width: M3_MIN_TOUCH_TARGET,
    height: M3_MIN_TOUCH_TARGET,
    borderRadius: M3_MIN_TOUCH_TARGET / 2,
  },
});
