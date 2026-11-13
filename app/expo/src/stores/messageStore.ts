import { create } from 'zustand';
import type { Message, MessageAction, MessageType } from '@/components/MessageToast.types';

export interface ShowMessageOptions {
  action?: MessageAction;
  onTimeout?: () => void;
}

interface MessageState {
  message: Message | null;
  showMessage: (text: string, type?: MessageType, options?: ShowMessageOptions) => void;
  clearMessage: () => void;
}

export const useMessageStore = create<MessageState>((set, get) => ({
  message: null,
  showMessage: (text: string, type: MessageType = 'info', options?: ShowMessageOptions) => {
    // 新消息顶掉旧消息时,旧消息视为自然结束(撤销类操作需要据此提交)
    get().message?.onTimeout?.();
    set({ message: { text, type, ...options } });
  },
  clearMessage: () => {
    set({ message: null });
  },
}));
