import { useEffect } from 'react';
import { useWindowDimensions } from 'react-native';

import { useHomeController } from './useHomeController';
import { getLayoutMode } from '@/hooks/useLayoutMode';
import { HomeCompactView } from './HomeCompactView';
import { HomeExpandedView } from './HomeExpandedView';
import { getHomeSearchSlots } from './android/homeSearchSlots';
import { getHomeHistoryCollection } from './android/homeHistoryCollection';
import type { HomeViewProps } from './HomeView.types';




import { NAVIGATION_RAIL_WIDTH } from '@/components/android/mainNavigationMetrics';

/**
 * Android 首页。两级布局:
 * - compact  : 手机(含横屏窄场景)/ 分屏 —— 单栏(HomeCompactView)。
 * - expanded : 平板 / 大屏 —— 方案 B 三栏工作台 · inset(HomeExpandedView)。左侧已是应用级
 *   navigation rail。
 *
 * 两级布局都只在搜索视图里提供筛选(getHomeSearchSlots):首页本身不筛选,永远是完整历史。
 *
 * Android 的 gutter/pane 底色走 M3 表面色阶:gutter=background、浮起面板=surfaceHigh。
 * 取 surfaceHigh(而非仅高半阶的 surfaceLow)是为了让面板在浅色下也明显浮起——
 * surfaceLow 与 background 在 light 下仅差 ~2%,肉眼几乎分不出。网格卡片仍是 surfaceLow,
 * 与手机端一致。
 */
export function HomeView({
  onOpenSettings,
  onOpenAbout,
  onImmersiveModeChange,
  addMenuOpenSignal,
}: HomeViewProps) {
  const c = useHomeController(onOpenSettings, { clearFiltersOnCloseSearch: true });

  const { width: screenWidth } = useWindowDimensions();
  const mode = getLayoutMode(screenWidth);
  const immersive = c.isSearching || c.isSelectMode;
  const search = getHomeSearchSlots(c);

  useEffect(() => {
    onImmersiveModeChange?.(immersive);
  }, [immersive, onImmersiveModeChange]);

  useEffect(() => () => onImmersiveModeChange?.(false), [onImmersiveModeChange]);


  if (mode === 'compact') {
    return (
      <HomeCompactView
        c={c}
        screenWidth={screenWidth}
        refreshTintColor={c.theme.colors.accent}
        // 网格延伸到悬浮导航胶囊与 FAB 之下(二者已计入 bottom inset),末行需滚出其上
        gridBottomPadding={c.insets.bottom + 80}
        addMenuOpenSignal={addMenuOpenSignal}
        search={search}
        renderCollection={getHomeHistoryCollection(c)}
      />
    );
  }

  return (
    <HomeExpandedView
      c={c}
      // expanded 时主页面左侧是应用级 navigation rail(含横屏左侧系统栏),首页只占剩余宽度
      screenWidth={screenWidth - NAVIGATION_RAIL_WIDTH - c.insets.left}
      refreshTintColor={c.theme.colors.accent}
      gutterColor={c.theme.colors.background as string}
      paneColor={c.theme.colors.surfaceHigh}
      filterPlacement="search"
      search={search}
    />
  );
}
