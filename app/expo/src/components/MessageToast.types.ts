export type MessageType = 'success' | 'error' | 'info';

/** 消息附带的单个操作(如「撤销」「更新」)。仅 Android Snackbar 渲染;iOS toast 忽略。 */
export interface MessageAction {
  label: string;
  onPress: () => void;
}

export interface Message {
  text: string;
  type: MessageType;
  action?: MessageAction;
  /** 消息自然消失(未点操作)时回调——撤销类操作据此提交真正的删除 */
  onTimeout?: () => void;
}

export interface MessageToastProps {
  message: Message | null;
  /** 动画播完(或被打断)后回调,宿主借此清空 message 状态 */
  onMessageShown: () => void;
  /**
   * toast 顶边距所在容器顶部的距离。不传时按「无导航头的全屏场景」自取平台默认
   * (含状态栏高度);设置页等 scene 在原生导航头下方的场景应传入小偏移。仅 iOS 使用。
   */
  topOffset?: number;
  /**
   * 提示底边距容器底部的距离。Android Snackbar 不传时为导航栏高度 + 16,首页需要抬到
   * FAB / 多选底栏之上;iOS 传入时改为底部玻璃胶囊(首页),不传时为顶部 toast。
   */
  bottomOffset?: number;
}
