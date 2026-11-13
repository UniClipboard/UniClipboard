import React from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import type { ColorScheme } from '@/theme/colors';
import { m3Type } from '@/theme/m3Typography';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import { M3IconButton } from './M3IconButton';

export const DETAIL_TOOLBAR_HEIGHT = 64;
/** 工具栏与屏幕底部(安全区之上)的距离 */
export const TOOLBAR_MARGIN = 24;

interface DetailFloatingToolbarProps {
  primary: ActionMenuItem | null;
  quick: ActionMenuItem[];
  colors: ColorScheme;
  bottomInset: number;
}

/**
 * 全屏详情页底部的 M3 浮动工具栏:左侧一组图标动作,右侧是突出的主动作(复制)。
 * 悬浮在内容之上、水平居中,内容区需自行留出 DETAIL_TOOLBAR_HEIGHT + 底部边距。
 */
export function DetailFloatingToolbar({
  primary,
  quick,
  colors,
  bottomInset,
}: DetailFloatingToolbarProps) {
  return (
    <View
      pointerEvents="box-none"
      style={[styles.container, { bottom: bottomInset + TOOLBAR_MARGIN }]}
    >
      {quick.length > 0 ? (
        <View style={[styles.toolbar, { backgroundColor: colors.surfaceHigh }]}>
          {quick.map((action) => (
            <M3IconButton
              key={action.key}
              testID={`detail-quick-${action.key}`}
              icon={action.icon as keyof typeof Ionicons.glyphMap}
              accessibilityLabel={action.label}
              onPress={action.onPress}
              colors={colors}
              iconColor={colors.textPrimary}
            />
          ))}
        </View>
      ) : null}
      {primary ? (
        <Pressable
          testID="detail-primary-action"
          onPress={primary.onPress}
          accessibilityRole="button"
          accessibilityLabel={primary.label}
          android_ripple={{ color: colors.fillSecondary as string }}
          style={[styles.fab, { backgroundColor: colors.accent }]}
        >
          <Ionicons name={primary.icon as never} size={22} color={colors.onAccent} />
          <Text style={[styles.fabLabel, { color: colors.onAccent }]} numberOfLines={1}>
            {primary.label}
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    position: 'absolute',
    left: 16,
    right: 16,
    flexDirection: 'row',
    justifyContent: 'center',
    alignItems: 'center',
    gap: 8,
  },
  toolbar: {
    height: DETAIL_TOOLBAR_HEIGHT,
    paddingHorizontal: 8,
    borderRadius: DETAIL_TOOLBAR_HEIGHT / 2,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    elevation: 3,
  },
  fab: {
    height: DETAIL_TOOLBAR_HEIGHT,
    paddingLeft: 20,
    paddingRight: 24,
    borderRadius: 20,
    overflow: 'hidden',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    elevation: 3,
  },
  fabLabel: {
    ...m3Type.titleMedium,
  },
});
