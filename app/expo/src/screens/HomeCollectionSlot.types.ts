import type React from 'react';
import type { RefreshControlProps } from 'react-native';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';

/**
 * 共享的 Compact 布局交给平台历史呈现(如 Android 分组列表)的摆放参数。
 * 平台不注入呈现时,Compact 布局使用卡片网格。
 */
export interface HomeCollectionSlot {
  /** 内容顶部留白(不含 header) */
  paddingTop: number;
  /** 内容底部留白,需让出 FAB / 多选底栏 */
  paddingBottom: number;
  /** 内容顶部、随内容滚动的页眉(如搜索结果数) */
  header?: React.ReactNode;
  /** 下拉刷新;下拉关闭生效时不提供 */
  refreshControl?: React.ReactElement<RefreshControlProps>;
  /** 在顶部下拉关闭的过程汇报 */
  pullToDismiss?: PullToDismissHandlers;
}

export type RenderHomeCollection = (slot: HomeCollectionSlot) => React.ReactNode;
