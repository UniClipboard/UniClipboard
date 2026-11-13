import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { AppButton, AppHost } from '@/components/ui';
import { m3Type } from '@/theme/m3Typography';
import type { HomeController } from '../useHomeController';
import type { HomeSearchSlots } from '../HomeSearchSlots.types';
import { HomeSearchFilterBar } from './HomeSearchFilterBar';
import { HomeSearchShortcuts } from './HomeSearchShortcuts';

const RESULT_HEADER_HEIGHT = 28;

/**
 * Android 搜索视图的两种形态:
 * - 空查询且无筛选:快捷筛选面板覆盖网格;
 * - 有关键词或筛选:顶栏下固定筛选行 + 网格页眉结果数;空结果时给出「清除筛选」。
 * 不在搜索态时首页没有任何筛选 UI。
 */
export function getHomeSearchSlots(c: HomeController): HomeSearchSlots | undefined {
  if (!c.isSearching) return undefined;
  const hasKeyword = c.searchText.trim().length > 0;
  if (!hasKeyword && !c.hasActiveFilters) {
    return { gridOverlay: <HomeSearchShortcuts c={c} /> };
  }

  const { colors } = c.theme;
  return {
    topBarAccessory: <HomeSearchFilterBar c={c} />,
    gridHeader: {
      height: RESULT_HEADER_HEIGHT,
      node: (
        <View style={styles.header}>
          <Text
            testID="history-search-result-count"
            accessibilityLiveRegion="polite"
            numberOfLines={1}
            style={[styles.headerText, { color: colors.textSecondary }]}
          >
            {c.isHistoryLoading
              ? c.t('search.loading')
              : c.t('search.resultCount', { count: c.resultCount })}
          </Text>
        </View>
      ),
    },
    emptyAction: c.hasActiveFilters ? (
      <AppHost matchContents style={styles.emptyAction}>
        <AppButton
          testID="history-search-empty-clear-filters"
          variant="tonal"
          title={c.t(hasKeyword ? 'search.clearFiltersKeepQuery' : 'search.clearFilters')}
          onPress={c.handleClearFilters}
        />
      </AppHost>
    ) : undefined,
  };
}

const styles = StyleSheet.create({
  header: {
    height: RESULT_HEADER_HEIGHT,
    justifyContent: 'flex-end',
    paddingHorizontal: 16,
  },
  headerText: { ...m3Type.bodyMedium },
  emptyAction: { marginTop: 8 },
});
