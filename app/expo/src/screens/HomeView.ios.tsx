import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { KeyboardAvoidingView, StyleSheet, View, useWindowDimensions } from 'react-native';
import Animated, { FadeIn, FadeOutDown } from 'react-native-reanimated';
import { DefaultTopBar, SelectModeTopBar } from '@/components/HomeTopBar';
import type { AnimatedCardGridHandle } from '@/components/AnimatedCardGrid';
import { iosColors } from '@/theme/iosDesignTokens';
import { useHomeController, type HomeController } from './useHomeController';
import { getLayoutMode } from '@/hooks/useLayoutMode';
import { HomeCompactView } from './HomeCompactView';
import { HomeExpandedView } from './HomeExpandedView';
import { HomeOverlays } from './HomeOverlays';
import { HomeSearchDock } from './ios/HomeSearchDock';
import { HomeLargeTitle, HOME_LARGE_TITLE_HEIGHT } from './ios/HomeLargeTitle';
import { getHomeHistoryCollection } from './ios/homeHistoryCollection';
import { dismissHomeSearch, getHomeSearchSlots } from './ios/homeSearchSlots';
import { HomeSearchFilterBar } from './ios/HomeSearchFilterBar';
import { useHomeBaseItems } from './ios/useHomeBaseItems';
import { useSearchPullToDismiss } from './ios/useSearchPullToDismiss';
import type { HomeViewProps } from './HomeView.types';

/** 顶栏按钮行高度(玻璃胶囊 44pt) */
const TOP_BAR_ROW_HEIGHT = 44;

/** 搜索层进场淡入;退场时带着最后一帧淡出下移,露出下面的首页 */
const SEARCH_LAYER_ENTERING = FadeIn.duration(160);
const SEARCH_LAYER_EXITING = FadeOutDown.duration(240);

/**
 * 列表末尾为原生标签栏留出的空间:iOS 26 悬浮标签栏约 62pt 高,底边贴近 home indicator,
 * 再留 16pt 间隔。剪贴板标签关闭了原生的 ScrollView inset 自动调整,以这里为准。
 */
function tabBarClearance(safeBottom: number): number {
  return Math.max(12, safeBottom - 6) + 62 + 16;
}

/**
 * iOS 首页。两级布局:
 * - compact  : iPhone(含横屏)/ iPad 分屏 —— 单栏(HomeCompactView)。
 * - expanded : iPad 全屏 / 大屏 —— 方案 B 三栏工作台 · inset(HomeExpandedView)。
 *
 * 单栏:右上是 Liquid Glass 按钮组(「+」添加菜单、「⋯」选择 / 显示方式),其下是随内容滚动的
 * 大标题;搜索入口是标签栏的搜索圆钮(`searchRequestId`),搜索 / 多选时收起标签栏。
 * 搜索是盖在首页上的独立一层:首页在下面保持进入搜索前的样子,退出搜索时搜索层退场即露出首页,
 * 不需要等首页历史重新查询。
 *
 * iOS 的 gutter/pane 底色走系统分组背景:gutter=systemGroupedBackground、
 * 浮起面板=secondarySystemGroupedBackground(网格区同为该面板色,中间区域是一个整体白面板)。
 */
export function HomeView({ onOpenSettings, onImmersiveModeChange, searchRequestId = 0 }: HomeViewProps) {
  const { width: screenWidth } = useWindowDimensions();
  const mode = getLayoutMode(screenWidth);
  // iPhone 的筛选只在搜索视图里;iPad 的筛选在常驻侧栏,退出搜索不清空
  const c = useHomeController(onOpenSettings, { clearFiltersOnCloseSearch: mode === 'compact' });
  // 搜索、多选、详情页打开时收起标签栏
  const immersive = c.isSearching || c.isSelectMode || c.detailPageItem != null;
  const handledSearchRequest = useRef(searchRequestId);
  const { openSearch, closeSearch } = c;
  const dismissSearch = useCallback(() => dismissHomeSearch(closeSearch), [closeSearch]);
  const pullToDismiss = useSearchPullToDismiss(c.isSearching, dismissSearch);
  const baseItems = useHomeBaseItems(c);
  // 搜索层的列表用自己的句柄;c.listRef 始终留给首页(新内容到来时滚回顶部)
  const searchListRef = useRef<AnimatedCardGridHandle>(null);
  const frozenHome = useRef<React.ReactElement | null>(null);

  useEffect(() => {
    onImmersiveModeChange?.(immersive);
  }, [immersive, onImmersiveModeChange]);
  useEffect(() => () => onImmersiveModeChange?.(false), [onImmersiveModeChange]);

  // 标签栏搜索圆钮:每次按下进入搜索(挂载时的初值不算一次请求)
  useEffect(() => {
    if (searchRequestId === handledSearchRequest.current) return;
    handledSearchRequest.current = searchRequestId;
    openSearch();
  }, [searchRequestId, openSearch]);

  const addActions = useMemo(
    () => ({
      onTakePhoto: c.handleTakePhoto,
      onPickImage: c.handleUploadImage,
      onPickFile: c.handleUploadFile,
      onUploadClipboard: c.handleUpload,
      onSync: c.handleSyncHistory,
    }),
    [c.handleTakePhoto, c.handleUploadImage, c.handleUploadFile, c.handleUpload, c.handleSyncHistory]
  );

  if (mode === 'compact') {
    const topBarStyle = [
      styles.topBar,
      { paddingTop: c.insets.top, height: c.insets.top + TOP_BAR_ROW_HEIGHT },
    ];
    const selectModeTopBar = (
      <SelectModeTopBar
        count={c.selectedIds.size}
        allSelected={c.allSelected}
        onSelectAll={c.handleSelectAll}
        onDone={c.exitSelectMode}
        theme={c.theme}
      />
    );

    // 网格页眉贴在内容容器边缘,列表页眉在 16pt 内容留白之内;页眉文字都与屏幕左缘保持 20pt
    const headerInset = c.historyLayout === 'grid' ? 20 : 4;

    // 搜索期间首页沿用进入搜索前的那一帧,输入关键词、切换筛选都不再重渲染它
    if (!c.isSearching || !frozenHome.current) {
      const home: HomeController = { ...c, items: baseItems, isSearching: false };
      frozenHome.current = (
        <HomeCompactView
          c={home}
          screenWidth={screenWidth}
          refreshTintColor={undefined}
          overlayTopBarHeight={c.insets.top + TOP_BAR_ROW_HEIGHT}
          gridBottomPadding={tabBarClearance(c.insets.bottom)}
          showAddActionsFab={false}
          renderOverlays={false}
          search={{
            gridHeader: {
              height: HOME_LARGE_TITLE_HEIGHT,
              node: <HomeLargeTitle title={c.t('nav.clipboard')} horizontalInset={headerInset} />,
            },
          }}
          renderCollection={getHomeHistoryCollection(home)}
          topBar={
            <View pointerEvents="box-none" style={topBarStyle}>
              {home.isSelectMode ? (
                selectModeTopBar
              ) : (
                <DefaultTopBar
                  onSearch={c.openSearch}
                  onSettings={c.onOpenSettings}
                  onSelectMode={() => {
                    c.setIsSelectMode(true);
                    c.clearSelection();
                  }}
                  historyLayout={c.historyLayout}
                  onHistoryLayoutChange={c.setHistoryLayout}
                  addActions={addActions}
                  theme={c.theme}
                />
              )}
            </View>
          }
        />
      );
    }

    const searchC: HomeController = { ...c, listRef: searchListRef };
    const search = getHomeSearchSlots(searchC, headerInset, pullToDismiss.handlers);

    return (
      <View style={styles.fill}>
        {/* 被搜索层盖住时,首页不接收触摸,也不出现在读屏里 */}
        <View
          style={styles.fill}
          pointerEvents={c.isSearching ? 'none' : 'auto'}
          accessibilityElementsHidden={c.isSearching}
          importantForAccessibility={c.isSearching ? 'no-hide-descendants' : 'auto'}
        >
          {frozenHome.current}
        </View>
        {c.isSearching ? (
          <Animated.View
            style={StyleSheet.absoluteFill}
            entering={SEARCH_LAYER_ENTERING}
            exiting={SEARCH_LAYER_EXITING}
          >
            <KeyboardAvoidingView style={styles.fill} behavior="padding">
              <HomeCompactView
                c={searchC}
                screenWidth={screenWidth}
                refreshTintColor={undefined}
                overlayTopBarHeight={c.insets.top + TOP_BAR_ROW_HEIGHT}
                gridBottomPadding={tabBarClearance(c.insets.bottom)}
                showAddActionsFab={false}
                renderOverlays={false}
                search={search}
                renderCollection={getHomeHistoryCollection(searchC)}
                topBar={
                  <View pointerEvents="box-none" style={topBarStyle}>
                    {c.isSelectMode ? (
                      selectModeTopBar
                    ) : // 筛选行只在结果视图里出现,空查询时是「建议」
                    search?.gridOverlay ? null : (
                      <View style={styles.filterBar}>
                        <HomeSearchFilterBar c={c} />
                      </View>
                    )}
                  </View>
                }
                bottomSearch={<HomeSearchDock c={c} style={pullToDismiss.dockStyle} />}
              />
            </KeyboardAvoidingView>
          </Animated.View>
        ) : null}
        <HomeOverlays c={c} />
      </View>
    );
  }

  return (
    <KeyboardAvoidingView style={styles.fill} behavior="padding">
      <HomeExpandedView
        c={c}
        screenWidth={screenWidth}
        refreshTintColor={undefined}
        gutterColor={iosColors?.systemGroupedBackground ?? (c.theme.colors.background as string)}
        paneColor={iosColors?.secondarySystemGroupedBackground ?? c.theme.colors.surfaceLow}
      />
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  topBar: { paddingHorizontal: 16 },
  filterBar: { paddingTop: 4 },
});
