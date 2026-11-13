import React from 'react';
import { useMessageStore } from '@/stores/messageStore';
import { MessageToast } from './MessageToast';

/**
 * 自订阅 messageStore 的 MessageToast：toast 出现/消失只重渲此组件，不波及宿主。
 *
 * RN Modal 呈现在独立原生窗口，会盖住主树里的 toast——Modal 型全屏浮层
 * （如分词浮层）需要在 Modal 内部再挂一份，浮层开着期间发出的消息才能
 * 显示在最上层。多实例并存无碍：同一条消息各自播动画，clearMessage 幂等。
 */
export function ConnectedMessageToast({
  topOffset,
  bottomOffset,
}: {
  topOffset?: number;
  bottomOffset?: number;
}) {
  const message = useMessageStore((s) => s.message);
  const clearMessage = useMessageStore((s) => s.clearMessage);
  return (
    <MessageToast
      message={message}
      onMessageShown={clearMessage}
      topOffset={topOffset}
      bottomOffset={bottomOffset}
    />
  );
}
