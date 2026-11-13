import React, { useCallback, useMemo } from 'react';
import { View, Text, StyleSheet, RefreshControl, type ColorValue } from 'react-native';
import Ionicons from '@expo/vector-icons/Ionicons';
import { AnimatedCardGrid } from '@/components/AnimatedCardGrid';
import { ClipboardCard } from '@/components/ClipboardCard';
import { ClipboardItem } from '@/types/clipboard';
import { computeGridMetrics } from '@/utils/gridLayout';
import type { HomeController } from './useHomeController';
import type { HomeSearchSlots } from './HomeSearchSlots.types';
import { iosColors, iosDimensions } from '@/theme/iosDesignTokens';

const GRID_SPACING = 12;
const GRID_PADDING = 16;

/**
 * Expanded 双栏的左栏:自适应多列历史网格。列数按左栏实测宽度反推(computeGridMetrics,
 * 目标卡宽 160–210pt),因此在不同平板宽度 / 左栏占比下都保持合理密度。
 *
 * 与 Compact 的关键差异:tap 不再直接复制,而是把该项设为右栏详情(master-detail 语义);
 * 复制改由右栏详情面板的显式动作承担。多选模式下 tap 仍是勾选。
 */
export function HomeMasterGrid({
  c,
  paneWidth,
  onSelectItem,
  showDetailSelection = true,
  refreshTintColor,
  header,
  emptyAction,
}: {
  c: HomeController;
  paneWidth: number;
  onSelectItem?: (item: ClipboardItem) => void;
  showDetailSelection?: boolean;
  refreshTintColor?: ColorValue;
  /** 网格内容顶部、随内容滚动的页眉(如搜索结果数) */
  header?: HomeSearchSlots['gridHeader'];
  /** 空结果状态下的主操作 */
  emptyAction?: React.ReactNode;
}) {
  const { theme, items, selectedIds, isSelectMode, detailItem } = c;

  const { numColumns, cardSize } = useMemo(
    () => computeGridMetrics(paneWidth, GRID_PADDING, GRID_SPACING, 2),
    [paneWidth]
  );

  const handlePress = useCallback(
    (item: ClipboardItem) => {
      if (isSelectMode) {
        c.toggleSelection(item.profileHash);
        return;
      }
      (onSelectItem ?? c.selectDetailItem)(item);
    },
    [isSelectMode, c.toggleSelection, c.selectDetailItem, onSelectItem]
  );

  const renderCard = useCallback(
    (item: ClipboardItem) => {
      const active = isSelectMode
        ? selectedIds.has(item.profileHash)
        : showDetailSelection && detailItem?.profileHash === item.profileHash;
      return (
        <ClipboardCard
          item={item}
          isLatest={item.profileHash === c.latestId}
          isSelected={active}
          isSelectMode={isSelectMode}
          onPress={handlePress}
          onDoublePress={c.handleItemDoublePress}
          onLongPress={c.handleItemLongPress}
          // 双栏卡片嵌在浮起面板(secondary)里,用第三层的 tertiarySystemGroupedBackground
          // 区分层级(light 柔灰 / dark 比面板亮一阶),即系统为「嵌在 secondary 面板里的内容块」
          // 设计的层级色;传入 surfaceColor 同时会关掉卡片的下投阴影(见 ClipboardCard.ios)。
          // 传具体色值而非 PlatformColor:卡片内部的 SVG 渐隐遮罩/折角缺口需要拿到字符串色值
          // 才能与卡底色严格一致(SVG Stop 不认 PlatformColor)。
          // Android 上 iosColors 为 null → undefined → 卡片走各自默认(surfaceLow/High 已自带对比)。
          surfaceColor={iosColors ? (theme.isDark ? '#2C2C2E' : '#F2F2F7') : undefined}
        />
      );
    },
    [
      c.latestId,
      c.handleItemLongPress,
      c.handleItemDoublePress,
      handlePress,
      selectedIds,
      isSelectMode,
      detailItem,
      showDetailSelection,
      theme.isDark,
    ]
  );

  if (items.length === 0) {
    return (
      <View style={styles.container}>
        {c.isInitialHistoryLoadComplete ? (
          <View style={styles.emptyState}>
            <View style={[styles.emptyIcon, { backgroundColor: theme.colors.surfaceHigh }]}>
              <Ionicons name={c.emptyContent.icon} size={30} color={c.emptyContent.tint} />
            </View>
            <Text style={[styles.emptyTitle, { color: theme.colors.textPrimary }]}>
              {c.emptyContent.title}
            </Text>
            <Text style={[styles.emptyDesc, { color: theme.colors.textSecondary }]}>
              {c.emptyContent.description}
            </Text>
            {emptyAction}
          </View>
        ) : null}
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <AnimatedCardGrid
        ref={c.listRef}
        items={items}
        numColumns={numColumns}
        cardSize={cardSize}
        renderCardSize={iosDimensions.gridAdaptiveMax}
        spacing={GRID_SPACING}
        paddingHorizontal={GRID_PADDING - GRID_SPACING / 2}
        paddingTop={8 + (header?.height ?? 0)}
        header={header?.node}
        paddingBottom={80}
        keyExtractor={c.keyExtractor}
        renderItem={renderCard}
        onVisibleItemsChanged={c.reportVisibleHistoryItems}
        onEndReached={c.loadMoreItems}
        refreshControl={
          <RefreshControl
            refreshing={c.refreshing}
            onRefresh={c.handleRefresh}
            tintColor={refreshTintColor}
            colors={[theme.colors.accent]}
          />
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  emptyState: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    gap: 10,
    paddingBottom: 32,
    paddingHorizontal: 24,
  },
  emptyIcon: {
    width: 64,
    height: 64,
    borderRadius: 32,
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 2,
  },
  emptyTitle: {
    fontSize: 18,
    fontWeight: '600',
    textAlign: 'center',
  },
  emptyDesc: {
    fontSize: 14,
    textAlign: 'center',
    lineHeight: 20,
  },
});
