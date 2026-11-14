import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { iosColors } from '@/theme/iosDesignTokens';

/** 大标题占用的高度(含上下留白),供列表头部留位 */
export const HOME_LARGE_TITLE_HEIGHT = 60;

/**
 * iOS 首页的大标题(34pt Bold),作为历史列表的页眉随内容滚动,与系统大标题的位置一致。
 * `horizontalInset` 抵消列表内容自身的左右留白,让标题与屏幕左缘保持 20pt。
 */
export function HomeLargeTitle({ title, horizontalInset }: { title: string; horizontalInset: number }) {
  return (
    <View style={[styles.container, { paddingHorizontal: horizontalInset }]}>
      <Text accessibilityRole="header" numberOfLines={1} style={styles.title}>
        {title}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { height: HOME_LARGE_TITLE_HEIGHT, justifyContent: 'center', paddingBottom: 4 },
  title: {
    fontSize: 34,
    lineHeight: 41,
    fontWeight: '700',
    letterSpacing: 0.37,
    color: iosColors?.label,
  },
});
