import type { ActionMenuItem } from '@/utils/actionMenuItems';
import type { Theme } from '@/theme';

export interface ClipboardDetailActionBarProps {
  primary: ActionMenuItem | null;
  quick: ActionMenuItem[];
  overflow: ActionMenuItem[];
  quickLabels: Record<string, string>;
  moreLabel: string;
  theme: Theme;
  /**
   * overflow 菜单是否展开(受控)。仅 Android 使用——其 overflow 是自绘动画浮层,需要外部
   * 托管开合态并配合 pane 的 dim backdrop。iOS 走系统 MenuView(原生 UIMenu),忽略这两个。
   */
  popoverOpen?: boolean;
  onPopoverOpenChange?: (open: boolean) => void;
}
