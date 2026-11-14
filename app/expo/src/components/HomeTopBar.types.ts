import type { useTheme } from '@/hooks/useTheme';
import type { ActionMenuItem } from '@/utils/actionMenuItems';
import type { HistoryLayout } from '@/hooks/useHistoryDisplaySettings';

/** 添加内容的入口动作(iOS 顶栏「+」原生菜单) */
export interface HomeAddActions {
  onTakePhoto: () => void;
  onPickImage: () => void;
  onPickFile: () => void;
  onUploadClipboard: () => void;
  onSync: () => void;
}

export interface DefaultTopBarProps {
  onSearch: () => void;
  onSettings: () => void;
  /** 显式进入多选(iOS「选择」按钮)。Android 由长按进入多选,不渲染此入口。 */
  onSelectMode: () => void;
  /**
   * 首页历史的显示方式与切换回调(Android 搜索栏尾部 ⋮、iOS 顶栏 ⋯ 的「显示方式」)。
   * 不传则不显示该菜单。
   */
  historyLayout?: HistoryLayout;
  onHistoryLayoutChange?: (layout: HistoryLayout) => void;
  /** 存在接收尝试时才传:顶栏菜单里出现「接收结果」入口,打开首页持有的接收结果弹层。 */
  receiveResults?: { count: number; onOpen: () => void };
  /** iOS:顶栏「+」菜单的添加动作;Android 由右下 FAB 承担,不传。 */
  addActions?: HomeAddActions;
  theme: ReturnType<typeof useTheme>['theme'];
}

export interface SearchTopBarProps {
  searchText: string;
  onChangeText: (t: string) => void;
  hasActiveFilters: boolean;
  resultCount?: number;
  isLoading?: boolean;
  onReset?: () => void;
  onClose: () => void;
  theme: ReturnType<typeof useTheme>['theme'];
}

export interface SelectModeTopBarProps {
  count: number;
  allSelected: boolean;
  onSelectAll: () => void;
  onDone: () => void;
  /**
   * 恰好选中一项时该项的内容类动作(Android 上下文操作栏的溢出菜单)。iOS 不使用。
   */
  itemActions?: ActionMenuItem[];
  theme: ReturnType<typeof useTheme>['theme'];
}
