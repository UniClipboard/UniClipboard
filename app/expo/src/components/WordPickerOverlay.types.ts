import type { CardAnchorRect } from './CardContextOverlay.types';

export interface WordPickerOverlayProps {
  /** 待分词的原文（超长会在内部截断展示） */
  text: string;
  /** 触发卡片在主窗口的位置，浮层从这里生长；null 时居中淡入 */
  anchor?: CardAnchorRect | null;
  /** 来源设备名，页面副标题展示「来自 X」；本机条目为空 */
  deviceName?: string | null;
  /** 把当前输出文本发送到其他设备；不传则不显示「发送到」（iOS 暂不提供） */
  onSendTo?: (text: string) => void;
  onDismiss: () => void;
}
