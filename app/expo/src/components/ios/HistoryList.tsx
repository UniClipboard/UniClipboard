import type { ViewToken } from 'react-native';
import React, { forwardRef, useCallback, useImperativeHandle, useMemo, useRef } from 'react';
import { StyleSheet, Text, View, type RefreshControlProps } from 'react-native';
import { FlashList, type FlashListRef, type ListRenderItemInfo } from '@shopify/flash-list';
import { useSharedValue } from 'react-native-reanimated';
import type { AnimatedCardGridHandle } from '@/components/AnimatedCardGrid';
import { useTheme } from '@/hooks/useTheme';
import type { ClipboardItem } from '@/types/clipboard';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';
import {
  buildHistoryListRows,
  formatHistoryDayLabel,
  type HistoryListRow as Row,
} from '@/utils/historyDayGroups';
import {
  HistoryListRow,
  HistoryOpenRowProvider,
  type HistoryListDensity,
  type HistoryListRowProps,
} from './HistoryListRow';

export interface HistoryListProps
  extends Pick<HistoryListRowProps, 'onPress' | 'onCopy' | 'onLongPress' | 'onDelete'> {
  items: ClipboardItem[];
  onVisibleItemsChanged?: (items: ClipboardItem[]) => void;
  keyExtractor: (item: ClipboardItem) => string;
  density: HistoryListDensity;
  selectedIds: ReadonlySet<string>;
  isSelectMode: boolean;
  paddingTop: number;
  paddingBottom: number;
  /** 列表顶部、随内容滚动的页眉(大标题、搜索结果数) */
  header?: React.ReactNode;
  refreshControl?: React.ReactElement<RefreshControlProps>;
  onEndReached?: () => void;
  /** 在顶部下拉关闭的过程汇报 */
  pullToDismiss?: PullToDismissHandlers;
}

/**
 * 按自然日分组的历史列表(iOS 分组列表):每天一个圆角白底分组,组内行以缩进分隔线隔开。
 * 与网格共用首页控制器,对外暴露与 AnimatedCardGrid 相同的 scrollToOffset 句柄。
 */
export const HistoryList = forwardRef<AnimatedCardGridHandle, HistoryListProps>(
  function HistoryList(
    {
      items,
      onVisibleItemsChanged,
      keyExtractor,
      density,
      selectedIds,
      isSelectMode,
      paddingTop,
      paddingBottom,
      header,
      refreshControl,
      onEndReached,
      pullToDismiss,
      onPress,
      onCopy,
      onLongPress,
      onDelete,
    },
    ref
  ) {
    const { theme } = useTheme();
    const listRef = useRef<FlashListRef<Row>>(null);
    const openRow = useSharedValue('');
    useImperativeHandle(
      ref,
      () => ({
        scrollToOffset: ({ offset, animated }) =>
          listRef.current?.scrollToOffset({ offset, animated: animated ?? true }),
      }),
      []
    );

    const onViewableItemsChanged = useCallback(({ viewableItems }: { viewableItems: ViewToken<Row>[] }) => {
      onVisibleItemsChanged?.(viewableItems.flatMap(({ item, isViewable }) =>
        isViewable && item.type === 'item' ? [item.item] : []));
    }, [onVisibleItemsChanged]);

    const rows = useMemo(() => buildHistoryListRows(items, keyExtractor), [items, keyExtractor]);

    const renderItem = useCallback(
      ({ item: row, index }: ListRenderItemInfo<Row>) => {
        if (row.type === 'header') {
          return (
            <Text
              accessibilityRole="header"
              style={[styles.dayHeader, index === 0 && styles.firstDayHeader, { color: theme.colors.textPrimary }]}
            >
              {formatHistoryDayLabel(row.dayStart)}
            </Text>
          );
        }
        return (
          <HistoryListRow
            // 回收复用时按条目重建行:滑动位移、「已复制」状态都不能串到另一条上
            key={row.key}
            rowKey={row.key}
            item={row.item}
            density={density}
            position={row.position}
            isSelected={selectedIds.has(row.item.profileHash)}
            isSelectMode={isSelectMode}
            onPress={onPress}
            onCopy={onCopy}
            onLongPress={onLongPress}
            onDelete={onDelete}
          />
        );
      },
      [theme.colors.textPrimary, density, selectedIds, isSelectMode, onPress, onCopy, onLongPress, onDelete]
    );

    return (
      <HistoryOpenRowProvider value={openRow}>
        <FlashList
          ref={listRef}
          data={rows}
          onViewableItemsChanged={onViewableItemsChanged}
          viewabilityConfig={HISTORY_VIEWABILITY}
          renderItem={renderItem}
          keyExtractor={rowKey}
          getItemType={rowType}
          extraData={renderItem}
          ListHeaderComponent={header ? <View>{header}</View> : undefined}
          contentContainerStyle={{ ...styles.content, paddingTop, paddingBottom }}
          refreshControl={refreshControl}
          onEndReached={onEndReached}
          onEndReachedThreshold={0.5}
          onScrollBeginDrag={() => {
            // 开始滚动时收起已展开的行
            openRow.value = '';
          }}
          onScroll={pullToDismiss && ((event) => pullToDismiss.onPull(event.nativeEvent.contentOffset.y))}
          scrollEventThrottle={pullToDismiss ? 16 : undefined}
          onScrollEndDrag={
            pullToDismiss && ((event) => pullToDismiss.onRelease(event.nativeEvent.contentOffset.y))
          }
          keyboardDismissMode="on-drag"
          keyboardShouldPersistTaps="handled"
        />
      </HistoryOpenRowProvider>
    );
  }
);

const HISTORY_VIEWABILITY = { itemVisiblePercentThreshold: 1, minimumViewTime: 0 };

const rowKey = (row: Row) => row.key;
const rowType = (row: Row) => row.type;

const styles = StyleSheet.create({
  content: { paddingHorizontal: 16 },
  dayHeader: {
    fontSize: 15,
    lineHeight: 20,
    fontWeight: '600',
    paddingHorizontal: 16,
    paddingTop: 22,
    paddingBottom: 7,
  },
  firstDayHeader: { paddingTop: 0 },
});
