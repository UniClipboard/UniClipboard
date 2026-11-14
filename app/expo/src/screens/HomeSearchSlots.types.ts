import type React from 'react';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';

/**
 * 平台首页向共享布局(Compact / Expanded)注入的搜索态内容。共享布局只负责摆放,
 * 不关心内容来自哪个平台;不传即无。
 */
export interface HomeSearchSlots {
  /** 顶栏下方的固定附加行(如搜索筛选行) */
  topBarAccessory?: React.ReactNode;
  /** 覆盖整个网格区的内容(如空查询时的快捷筛选) */
  gridOverlay?: React.ReactNode;
  /** 网格内容顶部、随内容滚动的页眉(如结果数);height 为其占用的高度 */
  gridHeader?: { node: React.ReactNode; height: number };
  /** 空结果状态下的主操作 */
  emptyAction?: React.ReactNode;
  /** 在历史顶部下拉退出搜索;提供时搜索态不挂下拉刷新,两者手势相同 */
  pullToDismiss?: PullToDismissHandlers;
}
