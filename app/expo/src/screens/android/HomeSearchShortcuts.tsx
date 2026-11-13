import React from 'react';
import {
  Keyboard,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  type ColorValue,
} from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { useTranslation } from 'react-i18next';
import { FilterChip } from '@/components/android/FilterChip';
import { getDisplayKindLabel, type DisplayKind } from '@/utils/displayKind';
import {
  getHistoryFilterDateOptions,
  getHistoryFilterSourceOptions,
  HISTORY_FILTER_KIND_OPTIONS,
} from '@/utils/historyFilterOptions';
import { m3Type } from '@/theme/m3Typography';
import type { HomeController } from '../useHomeController';
import { kindMenuIcon, SOURCE_ICON } from './HomeSearchFilterBar';

const KIND_COLUMNS = 3;

/**
 * 空查询的搜索视图:类型 / 时间 / 来源快捷项,排在键盘之上。点任一项即应用该筛选并收起
 * 键盘,视图随之切到结果(由 useHomeSearchSlots 按条件切换)。
 */
export function HomeSearchShortcuts({ c }: { c: HomeController }) {
  const { t } = useTranslation('home');
  const { colors } = c.theme;

  const apply = (action: () => void) => () => {
    Keyboard.dismiss();
    action();
  };

  const kindRows: Array<Array<DisplayKind | null>> = [];
  for (let i = 0; i < HISTORY_FILTER_KIND_OPTIONS.length; i += KIND_COLUMNS) {
    const row: Array<DisplayKind | null> = HISTORY_FILTER_KIND_OPTIONS.slice(i, i + KIND_COLUMNS);
    while (row.length < KIND_COLUMNS) row.push(null);
    kindRows.push(row);
  }

  return (
    <ScrollView
      testID="history-search-shortcuts"
      style={[StyleSheet.absoluteFill, { backgroundColor: colors.background }]}
      contentContainerStyle={styles.content}
      keyboardShouldPersistTaps="handled"
      keyboardDismissMode="on-drag"
    >
      <Section title={t('search.shortcut.kind')} color={colors.textSecondary}>
        <View style={styles.kindGrid}>
          {kindRows.map((row, rowIndex) => (
            <View key={rowIndex} style={styles.kindRow}>
              {row.map((kind, index) =>
                kind ? (
                  <Pressable
                    key={kind}
                    testID={`history-shortcut-kind-${kind}`}
                    onPress={apply(() => c.handleSelectFilterKind(kind))}
                    accessibilityRole="button"
                    android_ripple={{ color: colors.fillSecondary as string }}
                    style={[styles.kindTile, { backgroundColor: colors.surfaceLow }]}
                  >
                    <Ionicons
                      name={kindMenuIcon(kind) as keyof typeof Ionicons.glyphMap}
                      size={24}
                      color={colors.accent}
                    />
                    <Text style={[styles.kindLabel, { color: colors.textPrimary }]}>
                      {getDisplayKindLabel(kind)}
                    </Text>
                  </Pressable>
                ) : (
                  <View key={`empty-${index}`} style={styles.kindSpacer} />
                )
              )}
            </View>
          ))}
        </View>
      </Section>

      <Section title={t('search.shortcut.date')} color={colors.textSecondary}>
        <View style={styles.chips}>
          {getHistoryFilterDateOptions()
            .filter((option) => option.value !== 'all')
            .map((option) => (
              <FilterChip
                key={option.value}
                testID={`history-shortcut-date-${option.value}`}
                label={option.label}
                onPress={apply(() => c.setSelectedDateFilter(option.value))}
              />
            ))}
        </View>
      </Section>

      <Section title={t('search.shortcut.source')} color={colors.textSecondary}>
        <View style={styles.chips}>
          {getHistoryFilterSourceOptions().map((option) =>
            option.value === 'all' ? null : (
              <FilterChip
                key={option.value}
                testID={`history-shortcut-source-${option.value}`}
                label={option.label}
                icon={SOURCE_ICON[option.value]}
                onPress={apply(() => c.setSelectedSourceFilter(option.value))}
              />
            )
          )}
        </View>
      </Section>
    </ScrollView>
  );
}

function Section({
  title,
  color,
  children,
}: {
  title: string;
  color: ColorValue;
  children: React.ReactNode;
}) {
  return (
    <View style={styles.section}>
      <Text accessibilityRole="header" style={[styles.sectionTitle, { color }]}>
        {title}
      </Text>
      {children}
    </View>
  );
}

const styles = StyleSheet.create({
  content: { paddingHorizontal: 16, paddingTop: 8, paddingBottom: 24, gap: 24 },
  section: { gap: 12 },
  sectionTitle: { ...m3Type.titleSmall },
  kindGrid: { gap: 8 },
  kindRow: { flexDirection: 'row', gap: 8 },
  kindTile: {
    flex: 1,
    height: 72,
    borderRadius: 16,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    overflow: 'hidden',
  },
  kindSpacer: { flex: 1 },
  kindLabel: { ...m3Type.labelLarge },
  chips: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
});
