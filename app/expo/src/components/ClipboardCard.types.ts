import type { ColorValue } from 'react-native';
import type { ClipboardItem } from '@/types/clipboard';
import type { CardAnchorRect } from './CardContextOverlay.types';

export interface ClipboardCardProps {
  item: ClipboardItem;
  isLatest: boolean;
  isSelected?: boolean;
  isSelectMode?: boolean;
  onPress: (item: ClipboardItem) => void;
  /**
   * 双击回调(复制到系统剪贴板)。不传时单击立即触发、不等待双击;
   * 返回 true 表示复制成功,卡片会短暂显示「已复制」。
   */
  onDoublePress?: (item: ClipboardItem) => Promise<boolean> | void;
  /** anchor 是长按瞬间卡片的窗口坐标，measure 失败时为 null */
  onLongPress?: (item: ClipboardItem, anchor: CardAnchorRect | null) => void;
  /**
   * 卡片表面色覆盖。默认（compact/iPhone）不传，用各平台自己的默认卡片色。
   * iPad 双栏把卡片嵌在浮起面板里，需下沉一级以拉开与面板的对比，避免融为一体。
   */
  surfaceColor?: ColorValue;
}
