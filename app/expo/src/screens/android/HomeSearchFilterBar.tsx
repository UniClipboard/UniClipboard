import React from 'react';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { FilterChip } from '@/components/android/FilterChip';
import { OverflowMenu, type OverflowMenuItem } from '@/components/android/OverflowMenu';
import { getDisplayKindIcon, getDisplayKindLabel, type DisplayKind } from '@/utils/displayKind';
import {
  getHistoryDateFilterLabel,
  getHistoryFilterDateOptions,
  getHistoryFilterSourceOptions,
  getHistorySourceFilterLabel,
  HISTORY_FILTER_KIND_OPTIONS,
} from '@/utils/historyFilterOptions';
import { m3Type } from '@/theme/m3Typography';
import type { HomeController } from '../useHomeController';

export const SEARCH_FILTER_BAR_HEIGHT = 48;

export const SOURCE_ICON = { local: 'phone-portrait-outline', remote: 'desktop-outline' } as const;

export function kindMenuIcon(kind: DisplayKind): string {
  return `${getDisplayKindIcon(kind)}-outline`;
}

/**
 * 搜索结果视图的固定筛选行:类型 / 时间 / 来源三个下拉 chip,各自点开单选菜单,首项「全部」
 * 即清除该维度。行不随滚动收起;有筛选生效时尾部显示「清除」(只清筛选,保留关键词)。
 */
export function HomeSearchFilterBar({ c }: { c: HomeController }) {
  const { t } = useTranslation('history');
  const { colors } = c.theme;
  const kind = c.selectedFilterKinds[0] ?? null;
  const date = c.selectedDateFilter;
  const source = c.selectedSourceFilter;

  const kindItems: OverflowMenuItem[] = [
    {
      key: 'kind-all',
      label: t('search.allTypes', { ns: 'home' }),
      icon: 'apps-outline',
      selected: kind === null,
      onPress: () => c.handleSelectFilterKind(null),
    },
    ...HISTORY_FILTER_KIND_OPTIONS.map((option) => ({
      key: `kind-${option}`,
      label: getDisplayKindLabel(option),
      icon: kindMenuIcon(option),
      selected: kind === option,
      onPress: () => c.handleSelectFilterKind(option),
    })),
  ];
  const dateItems: OverflowMenuItem[] = getHistoryFilterDateOptions().map((option) => ({
    key: `date-${option.value}`,
    label: option.value === 'all' ? t('search.anyTime', { ns: 'home' }) : option.label,
    selected: date === option.value,
    onPress: () => c.setSelectedDateFilter(option.value),
  }));
  const sourceItems: OverflowMenuItem[] = getHistoryFilterSourceOptions().map((option) => ({
    key: `source-${option.value}`,
    label: option.label,
    icon: option.value === 'all' ? 'apps-outline' : SOURCE_ICON[option.value],
    selected: source === option.value,
    onPress: () => c.setSelectedSourceFilter(option.value),
  }));

  return (
    <View testID="history-search-filter-bar" style={styles.row}>
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        keyboardShouldPersistTaps="handled"
        style={styles.scroll}
        contentContainerStyle={styles.chips}
      >
        <OverflowMenu
          align="start"
          items={kindItems}
          renderTrigger={(open) => (
            <FilterChip
              testID="history-filter-kind"
              dropdown
              selected={kind !== null}
              icon={kind ? kindMenuIcon(kind) : undefined}
              label={kind ? getDisplayKindLabel(kind) : t('filter.chip.kind')}
              onPress={open}
            />
          )}
        />
        <OverflowMenu
          align="start"
          items={dateItems}
          renderTrigger={(open) => (
            <FilterChip
              testID="history-filter-date"
              dropdown
              selected={date !== 'all'}
              label={date !== 'all' ? getHistoryDateFilterLabel(date) : t('filter.chip.date')}
              onPress={open}
            />
          )}
        />
        <OverflowMenu
          align="start"
          items={sourceItems}
          renderTrigger={(open) => (
            <FilterChip
              testID="history-filter-source"
              dropdown
              selected={source !== 'all'}
              icon={source !== 'all' ? SOURCE_ICON[source] : undefined}
              label={
                source !== 'all' ? getHistorySourceFilterLabel(source) : t('filter.chip.source')
              }
              onPress={open}
            />
          )}
        />
      </ScrollView>
      {c.hasActiveFilters ? (
        <Pressable
          testID="history-filter-clear"
          onPress={c.handleClearFilters}
          accessibilityRole="button"
          accessibilityLabel={t('search.clearFilters', { ns: 'home' })}
          android_ripple={{ color: colors.fillSecondary as string }}
          style={styles.clear}
        >
          <Text style={[styles.clearLabel, { color: colors.accent }]}>
            {t('search.clear', { ns: 'home' })}
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  row: {
    height: SEARCH_FILTER_BAR_HEIGHT,
    flexDirection: 'row',
    alignItems: 'center',
  },
  scroll: { flexGrow: 1, flexShrink: 1 },
  chips: { gap: 8, alignItems: 'center', paddingRight: 8 },
  clear: {
    height: 40,
    paddingHorizontal: 12,
    borderRadius: 20,
    justifyContent: 'center',
    overflow: 'hidden',
  },
  clearLabel: { ...m3Type.labelLarge },
});
