import React, { useCallback } from 'react';
import { View, Text, StyleSheet, Pressable, ScrollView } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { DisplayKind } from '@/utils/displayKind';
import type { HomeController } from './useHomeController';

/**
 * Expanded 三栏工作台的最左「导航轨」。把类型筛选从搜索弹层提到常驻栏 —— 平板横屏一直可见,
 * 少一步进搜索。选中态用 accentContainer 药丸(不描边),与卡片「选中即填充」的语言一致。
 *
 * 数据完全复用现有筛选管线:切换只改 `selectedFilterKinds`,首页的 debounce effect 会据此
 * 调 searchItems,与搜索框里的类型筛选是同一份状态(两处协同,不冲突)。「全部」= 清空类型筛选。
 *
 * 高度自适应:手机横屏进入 expanded 时可用高度仅 ~340pt,因此整列用 ScrollView 兜底,
 * 条目再多也不溢出。
 */

interface RailEntry {
  kind: DisplayKind;
  icon: string; // Ionicons base name(填充);未选用 `${icon}-outline`
}

const ENTRIES: RailEntry[] = [
  { kind: 'text', icon: 'document-text' },
  { kind: 'url', icon: 'link' },
  { kind: 'image', icon: 'image' },
  { kind: 'file', icon: 'document' },
  { kind: 'group', icon: 'folder' },
];

export function HomeFilterRail({ c }: { c: HomeController }) {
  const { theme } = c;
  const { colors } = theme;
  const selected = c.selectedFilterKinds;
  const allActive = selected.length === 0;

  const renderItem = useCallback(
    (label: string, iconBase: string, active: boolean, onPress: () => void, key: string) => (
      <Pressable
        key={key}
        onPress={onPress}
        accessibilityRole="button"
        accessibilityState={{ selected: active }}
        style={styles.item}
      >
        <View
          style={[
            styles.iconPill,
            { backgroundColor: active ? colors.accentContainer : 'transparent' },
          ]}
        >
          <Ionicons
            name={(active ? iconBase : `${iconBase}-outline`) as never}
            size={18}
            color={active ? colors.onAccentContainer : colors.textSecondary}
          />
        </View>
        <Text
          style={[styles.label, { color: active ? colors.accent : colors.textSecondary }]}
          numberOfLines={1}
        >
          {label}
        </Text>
      </Pressable>
    ),
    [colors.accentContainer, colors.onAccentContainer, colors.textSecondary, colors.accent]
  );

  return (
    <ScrollView contentContainerStyle={styles.content} showsVerticalScrollIndicator={false}>
      <Text style={[styles.group, { color: colors.textTertiary }]}>{c.t('rail.filters')}</Text>

      {renderItem(c.t('rail.all'), 'apps', allActive, c.handleClearFilters, 'all')}

      {ENTRIES.map((e) =>
        renderItem(
          c.t(`kind.${e.kind}`, { ns: 'history' }),
          e.icon,
          selected.includes(e.kind),
          () => c.handleToggleFilterKind(e.kind),
          e.kind
        )
      )}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  content: {
    paddingVertical: 10,
    paddingHorizontal: 6,
    gap: 2,
  },
  group: {
    fontSize: 10,
    fontWeight: '600',
    letterSpacing: 0.4,
    textTransform: 'uppercase',
    paddingHorizontal: 6,
    paddingTop: 4,
    paddingBottom: 6,
    textAlign: 'center',
  },
  item: {
    alignItems: 'center',
    gap: 4,
    paddingVertical: 7,
    borderRadius: 16,
  },
  iconPill: {
    width: 48,
    height: 32,
    borderRadius: 14,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  label: {
    fontSize: 10,
    fontWeight: '500',
  },
});
