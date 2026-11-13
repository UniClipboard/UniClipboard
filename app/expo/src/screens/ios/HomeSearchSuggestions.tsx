import React from 'react';
import { Keyboard, Pressable, ScrollView, StyleSheet, Text, View, type ColorValue } from 'react-native';
import { File, Image as ImageIcon, Laptop, Layers, Link, Smartphone, Type, type LucideIcon } from 'lucide-react-native';
import { useTranslation } from 'react-i18next';
import { iosColors, iosKindTints } from '@/theme/iosDesignTokens';
import { getDisplayKindLabel, type DisplayKind } from '@/utils/displayKind';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';
import {
  getHistoryFilterDateOptions,
  getHistoryFilterSourceOptions,
  HISTORY_FILTER_KIND_OPTIONS,
} from '@/utils/historyFilterOptions';
import type { HomeController } from '../useHomeController';

export const KIND_ICON: Record<DisplayKind, LucideIcon> = {
  text: Type,
  url: Link,
  image: ImageIcon,
  file: File,
  group: Layers,
};

const KIND_COLUMNS = 3;

/**
 * 空查询的搜索视图(iOS):「建议」— 类型 / 时间 / 来源设备快捷项,排在键盘与底部搜索框之上。
 * 点任一项即应用该筛选并收起键盘,视图随之切到结果;开始输入同样进入结果。
 * 在顶部下拉即可退出搜索(`pullToDismiss`)。
 */
export function HomeSearchSuggestions({
  c,
  pullToDismiss,
}: {
  c: HomeController;
  pullToDismiss: PullToDismissHandlers;
}) {
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
      style={[StyleSheet.absoluteFill, { backgroundColor: iosColors?.systemGroupedBackground }]}
      contentContainerStyle={[styles.content, { paddingTop: c.insets.top + 8 }]}
      keyboardShouldPersistTaps="handled"
      keyboardDismissMode="on-drag"
      alwaysBounceVertical
      scrollEventThrottle={16}
      onScroll={(event) => pullToDismiss.onPull(event.nativeEvent.contentOffset.y)}
      onScrollEndDrag={(event) => pullToDismiss.onRelease(event.nativeEvent.contentOffset.y)}
    >
      <Text accessibilityRole="header" style={[styles.title, { color: colors.textPrimary }]}>
        {t('search.suggestions')}
      </Text>

      <Section title={t('filter.chip.kind', { ns: 'history' })} color={colors.textSecondary}>
        <View style={styles.kindGrid}>
          {kindRows.map((row, rowIndex) => (
            <View key={rowIndex} style={styles.kindRow}>
              {row.map((kind, index) => {
                if (!kind) return <View key={`empty-${index}`} style={styles.kindSpacer} />;
                const Icon = KIND_ICON[kind];
                return (
                  <Pressable
                    key={kind}
                    testID={`history-shortcut-kind-${kind}`}
                    onPress={apply(() => c.handleSelectFilterKind(kind))}
                    accessibilityRole="button"
                    style={({ pressed }) => [
                      styles.kindTile,
                      { backgroundColor: pressed ? iosColors?.tertiarySystemFill : iosColors?.secondarySystemGroupedBackground },
                    ]}
                  >
                    <Icon size={22} color={iosKindTints[kind]} />
                    <Text style={[styles.kindLabel, { color: colors.textPrimary }]}>
                      {getDisplayKindLabel(kind)}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          ))}
        </View>
      </Section>

      <Section title={t('filter.chip.date', { ns: 'history' })} color={colors.textSecondary}>
        <View style={styles.chips}>
          {getHistoryFilterDateOptions()
            .filter((option) => option.value !== 'all')
            .map((option) => (
              <SuggestionChip
                key={option.value}
                testID={`history-shortcut-date-${option.value}`}
                label={option.label}
                onPress={apply(() => c.setSelectedDateFilter(option.value))}
                c={c}
              />
            ))}
        </View>
      </Section>

      <Section title={t('filter.chip.source', { ns: 'history' })} color={colors.textSecondary}>
        <View style={styles.chips}>
          {getHistoryFilterSourceOptions()
            .filter((option) => option.value !== 'all')
            .map((option) => (
              <SuggestionChip
                key={option.value}
                testID={`history-shortcut-source-${option.value}`}
                label={option.label}
                icon={option.value === 'local' ? Smartphone : Laptop}
                onPress={apply(() => c.setSelectedSourceFilter(option.value))}
                c={c}
              />
            ))}
        </View>
      </Section>
    </ScrollView>
  );
}

function Section({ title, color, children }: { title: string; color: ColorValue; children: React.ReactNode }) {
  return (
    <View style={styles.section}>
      <Text style={[styles.sectionTitle, { color }]}>{title}</Text>
      {children}
    </View>
  );
}

function SuggestionChip({
  testID,
  label,
  icon: Icon,
  onPress,
  c,
}: {
  testID: string;
  label: string;
  icon?: LucideIcon;
  onPress: () => void;
  c: HomeController;
}) {
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      accessibilityRole="button"
      style={({ pressed }) => [
        styles.chip,
        Icon && styles.chipWithIcon,
        { backgroundColor: pressed ? iosColors?.tertiarySystemFill : iosColors?.secondarySystemGroupedBackground },
      ]}
    >
      {Icon ? <Icon size={17} color={c.theme.colors.textSecondary} /> : null}
      <Text style={[styles.chipLabel, { color: c.theme.colors.textPrimary }]}>{label}</Text>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  content: { paddingHorizontal: 16, paddingBottom: 120, gap: 22 },
  title: { fontSize: 22, lineHeight: 28, fontWeight: '700', paddingHorizontal: 4 },
  section: { gap: 10 },
  sectionTitle: { fontSize: 13, fontWeight: '600', paddingHorizontal: 4 },
  kindGrid: { gap: 8 },
  kindRow: { flexDirection: 'row', gap: 8 },
  kindTile: {
    flex: 1,
    height: 64,
    borderRadius: 16,
    borderCurve: 'continuous',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 5,
  },
  kindSpacer: { flex: 1 },
  kindLabel: { fontSize: 13, fontWeight: '600' },
  chips: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  chip: {
    height: 36,
    paddingHorizontal: 14,
    borderRadius: 18,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  chipWithIcon: { paddingLeft: 10 },
  chipLabel: { fontSize: 15, fontWeight: '500' },
});
