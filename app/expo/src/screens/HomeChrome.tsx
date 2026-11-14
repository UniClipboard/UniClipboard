import React from 'react';
import { View, StyleSheet } from 'react-native';
import { DefaultTopBar, SearchTopBar, SelectModeTopBar } from '@/components/HomeTopBar';
import type { HomeController } from './useHomeController';

/**
 * 顶栏区域(三态:默认 / 搜索 / 多选)。Compact 与 Expanded 都把它铺在全宽顶部,
 * 因此抽成共享组件。各 TopBar 子组件本身已按平台拆分。`accessory` 是平台注入的顶栏附加行
 * (Android 搜索筛选行),固定在顶栏内、不随列表滚动。
 */
export function HomeTopBarArea({
  c,
  accessory,
  historyLayoutMenu = false,
}: {
  c: HomeController;
  accessory?: React.ReactNode;
  /** 默认态顶栏提供「显示方式」切换(只有 Compact 布局支持列表呈现) */
  historyLayoutMenu?: boolean;
}) {
  return (
    <View style={[styles.topBar, { paddingTop: c.insets.top + 4 }]}>
      {c.isSelectMode ? (
        <SelectModeTopBar
          count={c.selectedIds.size}
          allSelected={c.allSelected}
          onSelectAll={c.handleSelectAll}
          onDone={c.exitSelectMode}
          itemActions={c.selectionItemActions}
          theme={c.theme}
        />
      ) : c.isSearching ? (
        <SearchTopBar
          searchText={c.searchText}
          onChangeText={c.setSearchText}
          hasActiveFilters={c.hasActiveFilters}
          resultCount={c.resultCount}
          isLoading={c.isHistoryLoading}
          onReset={c.resetSearch}
          onClose={c.closeSearch}
          theme={c.theme}
        />
      ) : (
        <DefaultTopBar
          onSearch={c.openSearch}
          onSettings={c.onOpenSettings}
          historyLayout={historyLayoutMenu ? c.historyLayout : undefined}
          onHistoryLayoutChange={historyLayoutMenu ? c.setHistoryLayout : undefined}
          receiveResults={
            c.receiveEntryCount > 0
              ? { count: c.receiveEntryCount, onOpen: () => c.setReceiveResultsOpen(true) }
              : undefined
          }
          theme={c.theme}
          onSelectMode={() => {
            c.setIsSelectMode(true);
            c.clearSelection();
          }}
        />
      )}
      {accessory}
    </View>
  );
}

const styles = StyleSheet.create({
  topBar: {
    paddingHorizontal: 16,
    paddingBottom: 4,
  },
});
