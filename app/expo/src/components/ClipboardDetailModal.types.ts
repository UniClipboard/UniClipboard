import type { ColorValue } from 'react-native';
import type { HomeController } from '@/screens/useHomeController';
import type { ClipboardItem } from '@/types/clipboard';

export interface ClipboardDetailModalProps {
  visible: boolean;
  onDismiss: () => void;
  c: HomeController;
  /** 展示的条目;未传时回落到 `c.detailItem`(Expanded 窄屏的右栏选中项)。 */
  item?: ClipboardItem | null;
  containerColor?: ColorValue;
  /**
   * iOS 的呈现方式:page(默认)为推入式全屏详情页;sheet 为 iPad 窄屏选中项的原生 pageSheet。
   * Android 始终是全屏页面。
   */
  presentation?: 'page' | 'sheet';
}
