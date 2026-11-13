import type { ClipboardItem } from '@/types/clipboard';
import type { DisplayKind } from '@/utils/displayKind';
import type { ActionMenuItem } from '@/utils/actionMenuItems';

/** 卡片长按瞬间由 measureInWindow 得到的窗口坐标，浮层用它做锚定动画 */
export interface CardAnchorRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CardContextOverlayProps {
  /** null 时浮层不渲染 */
  item: ClipboardItem | null;
  displayKind: DisplayKind | null;
  /** measure 失败时为 null，浮层退化为从屏幕中心淡入 */
  anchor: CardAnchorRect | null;
  actionGroups: ActionMenuItem[][];
  onDismiss: () => void;
}
