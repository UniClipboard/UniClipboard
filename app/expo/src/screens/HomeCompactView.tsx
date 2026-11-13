import React, { useCallback } from 'react';
import { View, Text, StyleSheet, RefreshControl, StatusBar, type ColorValue } from 'react-native';
import type { SharedValue } from 'react-native-reanimated';
import Ionicons from '@expo/vector-icons/Ionicons';
import { iosColors } from '@/theme/iosDesignTokens';
import { AnimatedCardGrid } from '@/components/AnimatedCardGrid';
import {
  SelectModeBottomBar,
  SelectModeBottomBarContainer,
} from '@/components/HomeBottomBar';
import { AddActionsFab } from '@/components/AddActionsFab';
import { ClipboardCard } from '@/components/ClipboardCard';
import { ClipboardItem } from '@/types/clipboard';
import { HomeTopBarArea } from './HomeChrome';
import { HomeOverlays } from './HomeOverlays';
import type { HomeController } from './useHomeController';
import type { HomeSearchSlots } from './HomeSearchSlots.types';
import type { RenderHomeCollection } from './HomeCollectionSlot.types';

const GRID_SPACING = 12;
const GRID_PADDING = 16;
const NUM_COLUMNS = 2;

/**
 * 手机(以及 iPad 分屏 / 小平板竖屏)的单栏首页。这是旧 HomeView 的原始布局,行为零回归:
 * 固定 2 列平分屏宽的历史网格、tap 复制、long-press 上下文浮层、右下 FAB。
 * iOS/Android 共用同一份(与拆分前一致),平台差异全在各子组件内部;平台可经 `renderCollection`
 * 把网格换成其他历史呈现(Android 按日分组列表)。
 */
export function HomeCompactView({
  c,
  screenWidth,
  refreshTintColor,
  topBar,
  bottomSearch,
  search,
  overlayTopBarHeight = 0,
  gridBottomPadding = 80,
  addMenuOpenSignal,
  renderCollection,
  showAddActionsFab = true,
  renderOverlays = true,
}: {
  c: HomeController;
  screenWidth: number;
  refreshTintColor?: ColorValue;
  topBar?: React.ReactNode;
  bottomSearch?: React.ReactNode;
  /** 平台注入的搜索态内容(筛选行 / 快捷筛选 / 结果数 / 空结果操作) */
  search?: HomeSearchSlots;
  overlayTopBarHeight?: number;
  /** 默认态网格底部留白,需让出右下 FAB 与任何浮在网格底部的控件 */
  gridBottomPadding?: number;
  /** Android:添加菜单展开态的即时信号,透传给 FAB */
  addMenuOpenSignal?: SharedValue<boolean>;
  /** 平台注入的历史呈现(Android 分组列表);不传时为卡片网格 */
  renderCollection?: RenderHomeCollection;
  /** 默认态右下的添加 FAB;iOS 手机的添加入口在顶栏「+」菜单,不显示 */
  showAddActionsFab?: boolean;
  /** 渲染首页浮层(详情、长按菜单、提示等);叠放多个布局时由外层统一渲染一份 */
  renderOverlays?: boolean;
}) {
  const { theme, items, selectedIds, isSelectMode } = c;
  const backgroundColor = iosColors?.systemGroupedBackground ?? theme.colors.background;

  const cardSize =
    (screenWidth - GRID_PADDING * 2 - GRID_SPACING * (NUM_COLUMNS - 1)) / NUM_COLUMNS;
  const selectionBarClearance = c.insets.bottom + 76;
  const gridHeader = search?.gridHeader;
  const paddingBottom = isSelectMode ? selectionBarClearance : gridBottomPadding;
  const pullToDismiss = search?.pullToDismiss;
  // 下拉关闭与下拉刷新是同一个手势,前者生效时不挂刷新
  const refreshControl = pullToDismiss ? undefined : (
    <RefreshControl
      refreshing={c.refreshing}
      onRefresh={c.handleRefresh}
      tintColor={refreshTintColor}
      colors={[theme.colors.accent]}
      progressViewOffset={overlayTopBarHeight}
    />
  );

  const renderCard = useCallback(
    (item: ClipboardItem) => (
      <View style={styles.cardSlot}>
        <ClipboardCard
          item={item}
          isLatest={item.profileHash === c.latestId}
          isSelected={selectedIds.has(item.profileHash)}
          isSelectMode={isSelectMode}
          onPress={c.handleItemPress}
          onDoublePress={c.handleItemDoublePress}
          onLongPress={c.handleItemLongPress}
        />
      </View>
    ),
    [
      c.latestId,
      c.handleItemPress,
      c.handleItemDoublePress,
      c.handleItemLongPress,
      selectedIds,
      isSelectMode,
    ]
  );

  return (
    <View style={[styles.container, { backgroundColor }]}>
      <StatusBar
        barStyle={theme.isDark ? 'light-content' : 'dark-content'}
        backgroundColor="transparent"
        translucent
      />

      {overlayTopBarHeight === 0
        ? topBar ?? <HomeTopBarArea c={c} accessory={search?.topBarAccessory} historyLayoutMenu />
        : null}

      {/* Keep clearance in scrollable content so history can pass behind the toolbar. */}
      <View style={styles.gridArea}>
        {renderCollection ? (
          renderCollection({
            paddingTop: overlayTopBarHeight + 8,
            paddingBottom,
            header: gridHeader?.node,
            refreshControl,
            pullToDismiss,
          })
        ) : (
          <AnimatedCardGrid
            ref={c.listRef}
            items={items}
            numColumns={NUM_COLUMNS}
            cardSize={cardSize}
            spacing={GRID_SPACING}
            paddingHorizontal={GRID_PADDING - GRID_SPACING / 2}
            paddingTop={overlayTopBarHeight + 8 + (gridHeader?.height ?? 0)}
            header={gridHeader ? (
              <View style={{ paddingTop: overlayTopBarHeight }}>{gridHeader.node}</View>
            ) : undefined}
            paddingBottom={paddingBottom}
            keyExtractor={c.keyExtractor}
            renderItem={renderCard}
            onVisibleItemsChanged={c.reportVisibleHistoryItems}
            onEndReached={c.loadMoreItems}
            refreshControl={refreshControl}
            pullToDismiss={pullToDismiss}
          />
        )}
        {items.length === 0 && c.isInitialHistoryLoadComplete && (
          <View pointerEvents="box-none" style={[styles.emptyState, { top: overlayTopBarHeight }]}>
            <View style={[styles.emptyIcon, { backgroundColor: theme.colors.surfaceHigh }]}>
              <Ionicons name={c.emptyContent.icon} size={30} color={c.emptyContent.tint} />
            </View>
            <Text style={[styles.emptyTitle, { color: theme.colors.textPrimary }]}>
              {c.emptyContent.title}
            </Text>
            <Text style={[styles.emptyDesc, { color: theme.colors.textSecondary }]}>
              {c.emptyContent.description}
            </Text>
            {search?.emptyAction}
          </View>
        )}

        {search?.gridOverlay && overlayTopBarHeight > 0 ? (
          <View style={[StyleSheet.absoluteFill, { top: overlayTopBarHeight }]}>
            {search.gridOverlay}
          </View>
        ) : search?.gridOverlay}
      </View>

      {/* 多选底栏(默认态由右下 FAB 取代) */}
      {overlayTopBarHeight > 0 ? (
        <View pointerEvents="box-none" style={styles.topBarOverlay}>{topBar}</View>
      ) : null}
      {!isSelectMode && bottomSearch}
      {isSelectMode && (
        <SelectModeBottomBarContainer bottomInset={c.insets.bottom} theme={theme}>
          <SelectModeBottomBar
            disabled={selectedIds.size === 0}
            onCopy={c.handleBatchCopy}
            onShare={c.handleBatchShare}
            onSendTo={c.handleBatchSendTo}
            onDelete={c.handleBatchDelete}
            theme={theme}
          />
        </SelectModeBottomBarContainer>
      )}

      {/* 右下融合操作按钮 + 上传悬浮菜单(默认态) */}
      {showAddActionsFab && !isSelectMode && !c.isSearching && (
        <AddActionsFab
          open={c.showAddMenu}
          onOpenChange={c.setShowAddMenu}
          onTakePhoto={c.handleTakePhoto}
          onPickImage={c.handleUploadImage}
          onPickFile={c.handleUploadFile}
          onUploadClipboard={c.handleUpload}
          onSync={c.handleSyncHistory}
          theme={theme}
          openSignal={addMenuOpenSignal}
        />
      )}

      {renderOverlays ? <HomeOverlays c={c} /> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  topBarOverlay: { position: 'absolute', top: 0, left: 0, right: 0 },
  container: {
    flex: 1,
  },
  gridArea: {
    flex: 1,
  },
  cardSlot: {
    flex: 1,
    padding: GRID_SPACING / 2,
  },
  emptyState: {
    ...StyleSheet.absoluteFill,
    justifyContent: 'center',
    alignItems: 'center',
    gap: 10,
    paddingBottom: 40,
    paddingHorizontal: 40,
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
