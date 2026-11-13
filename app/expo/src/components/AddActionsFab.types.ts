import type { SharedValue } from 'react-native-reanimated';
import type { useTheme } from '@/hooks/useTheme';

/**
 * FAB 的边长(px)。两端实现与外部布局(如平板把 FAB 居中到 rail)共用同一个真源,
 * 避免各处各写一份 `56` 后悄悄漂移。
 */
export const FAB_SIZE = 56;

/**
 * FAB 及其展开菜单的水平锚定方向。
 * - `'end'`(默认):贴右下,菜单向左上展开——手机与竖屏平板的默认位置。
 * - `'start'`:贴左下,菜单向右上展开——横屏双栏时把 FAB 放到左侧 rail,避免与右侧
 *   详情面板底部的固定操作栏冲突。
 */
export type FabAnchor = 'start' | 'end';

export interface AddActionsFabProps {
  /** 悬浮菜单是否展开(Android 受控；iOS 原生 Menu 自行管理) */
  open: boolean;
  /** 更新 Android 受控状态；iOS 仅在执行菜单项时用于重置共享状态 */
  onOpenChange: (open: boolean) => void;
  /** 相机拍摄后上传 */
  onTakePhoto: () => void;
  /** 从相册选择图片上传 */
  onPickImage: () => void;
  /** 选择任意文件上传 */
  onPickFile: () => void;
  /** 上传当前系统剪贴板内容 */
  onUploadClipboard: () => void;
  /** 立即与服务器同步(iOS 融合入菜单末项;Android 由下拉刷新承担,菜单不提供) */
  onSync: () => void;
  theme: ReturnType<typeof useTheme>['theme'];
  /** 水平锚定方向,默认 `'end'`(右下)。 */
  anchor?: FabAnchor;
  /** 距锚定侧的水平内边距(px),默认 16。用于把 FAB 精确落到某一栏(如居中于 rail)。 */
  horizontalInset?: number;
  /**
   * Android:菜单展开态的即时信号(UI 线程)。点按时立即写入,不等父级重渲,供悬浮导航
   * 胶囊等同屏元素同步让位。iOS 原生 Menu 不使用。
   */
  openSignal?: SharedValue<boolean>;
}
