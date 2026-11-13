import type { ViewToken } from 'react-native';
import React, { forwardRef, useCallback, useImperativeHandle, useMemo, useRef } from 'react';
import { StyleSheet, Text, View, type RefreshControlProps } from 'react-native';
import { FlashList, type FlashListRef, type ListRenderItemInfo } from '@shopify/flash-list';
import type { AnimatedCardGridHandle } from '@/components/AnimatedCardGrid';
import { useTheme } from '@/hooks/useTheme';
import { m3Type } from '@/theme/m3Typography';
import type { ClipboardItem } from '@/types/clipboard';
import {
  buildHistoryListRows,
  formatHistoryDayLabel,
  type HistoryListRow as Row,
} from '@/utils/historyDayGroups';
import {
  HistoryListRow,
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
  /** 列表顶部、随内容滚动的页眉(如搜索结果数) */
  header?: React.ReactNode;
  refreshControl?: React.ReactElement<RefreshControlProps>;
  onEndReached?: () => void;
}

/**
 * 按自然日分组的历史列表(Android)。与网格共用首页控制器:同一份 items / 选择状态 /
 * 手势回调,只是换了呈现方式。对外暴露与 AnimatedCardGrid 相同的 scrollToOffset 句柄,
 * 控制器的「新内容置顶后回到顶部」无需区分两种视图。
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
      onPress,
      onCopy,
      onLongPress,
      onDelete,
    },
    ref
  ) {
    const { theme } = useTheme();
    const listRef = useRef<FlashListRef<Row>>(null);
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
              style={[
                styles.dayHeader,
                index === 0 && styles.firstDayHeader,
                { color: theme.colors.accent },
              ]}
            >
              {formatHistoryDayLabel(row.dayStart)}
            </Text>
          );
        }
        const isGroupEnd = row.position === 'single' || row.position === 'last';
        return (
          <View style={isGroupEnd ? undefined : styles.rowGap}>
            <HistoryListRow
              // 回收复用时按条目重建行:滑动位移、「已复制」状态都不能串到另一条上
              key={row.key}
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
          </View>
        );
      },
      [
        theme.colors.accent,
        density,
        selectedIds,
        isSelectMode,
        onPress,
        onCopy,
        onLongPress,
        onDelete,
      ]
    );

    return (
      <FlashList
        ref={listRef}
        data={rows}
        onViewableItemsChanged={onViewableItemsChanged}
        viewabilityConfig={HISTORY_VIEWABILITY}
        renderItem={renderItem}
        keyExtractor={rowKey}
        getItemType={rowType}
        extraData={renderItem}
        ListHeaderComponent={header ? <>{header}</> : undefined}
        contentContainerStyle={{ ...styles.content, paddingTop, paddingBottom }}
        refreshControl={refreshControl}
        onEndReached={onEndReached}
        onEndReachedThreshold={0.5}
        showsVerticalScrollIndicator={false}
        keyboardDismissMode="on-drag"
        keyboardShouldPersistTaps="handled"
      />
    );
  }
);

const HISTORY_VIEWABILITY = { itemVisiblePercentThreshold: 1, minimumViewTime: 0 };

const rowKey = (row: Row) => row.key;
const rowType = (row: Row) => row.type;

const styles = StyleSheet.create({
  content: { paddingHorizontal: 16 },
  dayHeader: {
    ...m3Type.titleSmall,
    paddingHorizontal: 4,
    paddingTop: 16,
    paddingBottom: 8,
  },
  firstDayHeader: { paddingTop: 8 },
  rowGap: { paddingBottom: 2 },
});
