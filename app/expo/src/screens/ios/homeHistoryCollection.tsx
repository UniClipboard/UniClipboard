import React from 'react';
import { HistoryList } from '@/components/ios/HistoryList';
import type { HomeController } from '../useHomeController';
import type { RenderHomeCollection } from '../HomeCollectionSlot.types';

const renderNothing: RenderHomeCollection = () => null;

/**
 * iOS 首页的历史呈现:显示方式为网格时交回共享 Compact 布局的卡片网格(返回 undefined),
 * 列表 / 紧凑列表时换成按日分组列表。显示方式尚未从本地读出时先不渲染,避免网格闪一下再切成列表。
 */
export function getHomeHistoryCollection(c: HomeController): RenderHomeCollection | undefined {
  if (c.isHistoryLayoutLoading) return renderNothing;
  if (c.historyLayout === 'grid') return undefined;
  const density = c.historyLayout === 'compact' ? 'compact' : 'comfortable';
  return (slot) => (
    <HistoryList
      ref={c.listRef}
      items={c.items}
      onVisibleItemsChanged={c.reportVisibleHistoryItems}
      keyExtractor={c.keyExtractor}
      density={density}
      selectedIds={c.selectedIds}
      isSelectMode={c.isSelectMode}
      paddingTop={slot.paddingTop}
      paddingBottom={slot.paddingBottom}
      header={slot.header}
      refreshControl={slot.refreshControl}
      pullToDismiss={slot.pullToDismiss}
      onEndReached={c.loadMoreItems}
      onPress={c.handleItemPress}
      onCopy={c.handleItemCopy}
      onLongPress={c.handleItemLongPress}
      onDelete={c.handleItemDelete}
    />
  );
}
