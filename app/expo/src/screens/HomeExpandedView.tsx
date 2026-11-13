import React, { useCallback, useMemo, useState } from 'react';
import { View, StyleSheet, StatusBar, type ColorValue } from 'react-native';
import { SelectModeBottomBar } from '@/components/HomeBottomBar';
import { AddActionsFab } from '@/components/AddActionsFab';
import { FAB_SIZE } from '@/components/AddActionsFab.types';
import { ClipboardDetailPane } from '@/components/ClipboardDetailPane';
import { ClipboardDetailModal } from '@/components/ClipboardDetailModal';
import { useHomeController } from './useHomeController';
import { HomeMasterGrid } from './HomeMasterGrid';
import { HomeFilterRail } from './HomeFilterRail';
import { HomeTopBarArea } from './HomeChrome';
import { HomeOverlays } from './HomeOverlays';
import type { HomeSearchSlots } from './HomeSearchSlots.types';
import { computeExpandedWorkspaceLayout } from '@/utils/gridLayout';
import type { ClipboardItem } from '@/types/clipboard';

type Controller = ReturnType<typeof useHomeController>;

const RAIL_WIDTH = 72;
const GUTTER = 12; // 面板之间 / 面板与屏幕边的缝隙
// side 模式把 FAB 水平居中到左侧 rail 底部:rail 左缘在 GUTTER 处,栏宽 RAIL_WIDTH。
const RAIL_FAB_INSET = GUTTER + (RAIL_WIDTH - FAB_SIZE) / 2;
// 无筛选栏时 FAB 锚在网格面板右下角(M3 惯例),与面板边缘留 16。
const PANE_FAB_INSET = GUTTER + 16;

/**
 * 平板首页 —— 自适应的筛选栏、历史网格与详情工作台。
 *
 * 足够宽时详情显示在网格右侧;窄平板保留筛选栏与完整网格,点击卡片后打开独立详情页。
 * 各区域使用浮起的圆角面板(paneColor),靠 GUTTER 缝隙分区,不画分割线。Android 面板取
 * surfaceHigh、卡片 surfaceLow ——
 * 卡片明显区别于面板背景(light 卡片更亮浮起、dark 卡片更暗内嵌,两主题都有清晰对比);
 * iOS 面板取 secondarySystemGroupedBackground、卡片取第三层的 tertiarySystemGroupedBackground
 * 并去掉阴影(在 HomeMasterGrid 传入),两主题都与面板有和谐对比。
 * gutter=background/systemGroupedBackground。
 *
 * 详情在首次点选前只显示轻量占位,避免旋转时在后台重排未使用的图片预览。
 * iOS / Android 差异在 gutter/pane 两个底色 token 与筛选位置,由各自平台的 HomeView 传入:
 * iOS 左侧为类型筛选栏;Android 左侧已是应用级 navigation rail,筛选只在搜索视图里
 * (与手机同一套 search 注入内容、同一份状态)。
 */
export function HomeExpandedView({
  c,
  screenWidth,
  refreshTintColor,
  gutterColor,
  paneColor,
  filterPlacement = 'rail',
  search,
}: {
  c: Controller;
  screenWidth: number;
  refreshTintColor?: ColorValue;
  /** 面板之间缝隙露出的底色(Android=background / iOS=systemGroupedBackground)。
   *  iOS 是 PlatformColor 对象,故用 string | object。 */
  gutterColor: string | object;
  /** 浮起面板的表面色(Android=surfaceHigh / iOS=secondarySystemGroupedBackground) */
  paneColor: string | object;
  /** 类型筛选放在左侧筛选栏(rail),还是只在搜索视图里(search)。 */
  filterPlacement?: 'rail' | 'search';
  /** 平台注入的搜索态内容(筛选行 / 快捷筛选 / 结果数 / 空结果操作) */
  search?: HomeSearchSlots;
}) {
  const { theme } = c;
  const filterRail = filterPlacement === 'rail';
  const workspace = useMemo(
    () => computeExpandedWorkspaceLayout(screenWidth, { filterRail }),
    [screenWidth, filterRail]
  );
  const [detailActivated, setDetailActivated] = useState(false);

  const handleSelectItem = useCallback(
    (item: ClipboardItem) => {
      c.setShowAddMenu(false);
      c.selectDetailItem(item);
      setDetailActivated(true);
    },
    [c.selectDetailItem, c.setShowAddMenu]
  );

  const panePlaceholder = paneColor as never;
  const showSideDetail = workspace.detailPlacement === 'side';

  return (
    <View style={[styles.container, { backgroundColor: gutterColor as never }]}>
      <StatusBar
        barStyle={theme.isDark ? 'light-content' : 'dark-content'}
        backgroundColor="transparent"
        translucent
      />

      <HomeTopBarArea c={c} accessory={search?.topBarAccessory} />

      <View style={[styles.split, { padding: GUTTER, gap: GUTTER }]}>
        {/* ── 筛选栏(浮起面板)── */}
        {filterRail && (
          <View style={[styles.pane, styles.railPane, { backgroundColor: panePlaceholder }]}>
            <HomeFilterRail c={c} />
          </View>
        )}

        {/* ── 历史网格(面板)── 卡片在面板上以层级色区分,三栏统一为面板 */}
        <View
          style={[
            styles.pane,
            styles.gridWell,
            { width: workspace.gridWidth },
            { backgroundColor: panePlaceholder },
          ]}
        >
          <HomeMasterGrid
            c={c}
            paneWidth={workspace.gridWidth}
            onSelectItem={handleSelectItem}
            showDetailSelection={detailActivated}
            refreshTintColor={refreshTintColor}
            header={search?.gridHeader}
            emptyAction={search?.emptyAction}
          />
          {search?.gridOverlay}
          {c.isSelectMode && (
            <View style={[styles.bottomBar, { paddingBottom: c.insets.bottom + 10 }]}>
              <SelectModeBottomBar
                disabled={c.selectedIds.size === 0}
                onCopy={c.handleBatchCopy}
                onShare={c.handleBatchShare}
                onDelete={c.handleBatchDelete}
                theme={theme}
              />
            </View>
          )}
        </View>

        {showSideDetail && (
          <View
            style={[
              styles.pane,
              styles.sideDetail,
              { width: workspace.detailWidth, backgroundColor: panePlaceholder },
            ]}
          >
            <ClipboardDetailPane c={c} item={detailActivated ? c.detailItem : null} />
          </View>
        )}
      </View>

      <ClipboardDetailModal
        visible={!showSideDetail && detailActivated}
        onDismiss={() => setDetailActivated(false)}
        c={c}
        containerColor={paneColor as ColorValue}
        presentation="sheet"
      />

      {/* 上传融合按钮:平板恒锚到左侧 rail 底部(横竖屏一致,菜单向右上展开),不再有右下角
          形态。overlay(竖屏)弹出详情时全屏 Modal 会盖住 rail,故此时隐藏 FAB;side 双栏详情
          常驻在右栏、不遮 rail,FAB 保持可见。 */}
      {!c.isSelectMode && !c.isSearching && !(detailActivated && !showSideDetail) && (
        <AddActionsFab
          open={c.showAddMenu}
          onOpenChange={c.setShowAddMenu}
          onTakePhoto={c.handleTakePhoto}
          onPickImage={c.handleUploadImage}
          onPickFile={c.handleUploadFile}
          onUploadClipboard={c.handleUpload}
          onSync={c.handleSyncHistory}
          theme={theme}
          anchor={filterRail ? 'start' : 'end'}
          horizontalInset={
            filterRail
              ? RAIL_FAB_INSET
              : PANE_FAB_INSET + (showSideDetail ? workspace.detailWidth + GUTTER : 0)
          }
        />
      )}

      <HomeOverlays c={c} />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  split: {
    flex: 1,
    flexDirection: 'row',
    position: 'relative',
  },
  pane: {
    borderRadius: 22,
    overflow: 'hidden',
  },
  railPane: {
    width: RAIL_WIDTH,
  },
  gridWell: {
    flexGrow: 0,
    flexShrink: 0,
    position: 'relative',
  },
  sideDetail: {
    position: 'absolute',
    top: GUTTER,
    right: GUTTER,
    bottom: GUTTER,
    zIndex: 2,
    elevation: 8,
  },
  bottomBar: {
    position: 'absolute',
    bottom: 0,
    left: 0,
    right: 0,
    paddingHorizontal: 16,
    paddingTop: 10,
  },
});
