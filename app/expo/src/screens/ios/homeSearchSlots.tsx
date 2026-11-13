import React from 'react';
import { Keyboard, StyleSheet, Text, View } from 'react-native';
import { AppButton, AppHost } from '@/components/ui';
import type { HomeController } from '../useHomeController';
import type { HomeSearchSlots } from '../HomeSearchSlots.types';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';
import { HomeSearchSuggestions } from './HomeSearchSuggestions';

const RESULT_HEADER_HEIGHT = 26;

/**
 * iOS 搜索视图的两种形态(筛选行由首页顶栏承接):
 * - 空查询且无筛选:「建议」覆盖网格;
 * - 有关键词或筛选:列表页眉显示结果数;空结果时给出「清除筛选」(保留关键词)。
 * `headerInset` 是结果数与列表内容左缘的距离,让它与屏幕左缘保持 20pt。
 * 两种形态都支持在顶部下拉关闭搜索(`pullToDismiss`)。
 */
export function getHomeSearchSlots(
  c: HomeController,
  headerInset: number,
  pullToDismiss: PullToDismissHandlers
): HomeSearchSlots | undefined {
  if (!c.isSearching) return undefined;
  const hasKeyword = c.searchText.trim().length > 0;
  if (!hasKeyword && !c.hasActiveFilters) {
    return {
      gridOverlay: <HomeSearchSuggestions c={c} pullToDismiss={pullToDismiss} />,
      pullToDismiss,
    };
  }

  return {
    pullToDismiss,
    gridHeader: {
      height: RESULT_HEADER_HEIGHT,
      node: (
        <View style={[styles.header, { paddingHorizontal: headerInset }]}>
          <Text
            testID="history-search-result-count"
            accessibilityLiveRegion="polite"
            numberOfLines={1}
            style={[styles.headerText, { color: c.theme.colors.textSecondary }]}
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

/** 退出搜索:收起键盘并关闭搜索层(底部关闭按钮与下拉关闭共用) */
export function dismissHomeSearch(closeSearch: () => void) {
  Keyboard.dismiss();
  closeSearch();
}

const styles = StyleSheet.create({
  header: { height: RESULT_HEADER_HEIGHT, justifyContent: 'flex-start' },
  headerText: { fontSize: 13, fontWeight: '600' },
  emptyAction: { marginTop: 10 },
});
