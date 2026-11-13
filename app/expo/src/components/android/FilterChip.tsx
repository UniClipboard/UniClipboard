import React from 'react';
import { Pressable, StyleSheet, Text } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { useTheme } from '@/hooks/useTheme';
import { m3Type } from '@/theme/m3Typography';

interface FilterChipProps {
  testID?: string;
  label: string;
  /** 生效态:只换填充色,不插对勾,切换时宽度不变 */
  selected?: boolean;
  /** 前导 Ionicons 图标名 */
  icon?: string;
  /** 下拉 chip(点开菜单):尾部 ▾,无障碍上报为 button + expanded 语义 */
  dropdown?: boolean;
  onPress: () => void;
}

/**
 * M3 filter chip(32dp 视觉高、上下 hitSlop 补足 48dp 触控目标、整块 ripple)。
 * 搜索筛选行的下拉 chip 与搜索快捷项共用这一个实现。
 */
export function FilterChip({
  testID,
  label,
  selected = false,
  icon,
  dropdown = false,
  onPress,
}: FilterChipProps) {
  const { colors } = useTheme().theme;
  const fg = selected ? colors.onAccentContainer : colors.textSecondary;
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      hitSlop={{ top: 8, bottom: 8 }}
      android_ripple={{ color: colors.fillSecondary as string }}
      accessibilityRole="button"
      accessibilityState={{ selected }}
      style={[
        styles.chip,
        icon ? styles.withIcon : null,
        dropdown ? styles.withTrailing : null,
        selected
          ? { backgroundColor: colors.accentContainer, borderColor: colors.accentContainer }
          : { borderColor: colors.separator },
      ]}
    >
      {icon ? (
        <Ionicons name={icon as keyof typeof Ionicons.glyphMap} size={18} color={fg} />
      ) : null}
      <Text style={[styles.label, { color: fg }]} numberOfLines={1}>
        {label}
      </Text>
      {dropdown ? <Ionicons name="chevron-down" size={18} color={fg} /> : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  chip: {
    height: 32,
    borderRadius: 8,
    borderWidth: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 16,
    overflow: 'hidden',
  },
  withIcon: { paddingLeft: 8 },
  withTrailing: { paddingRight: 8 },
  label: { ...m3Type.labelLarge },
});
