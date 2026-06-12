import { create } from 'zustand';

type MessageType = 'success' | 'error' | 'info';

interface MessageState {
  message: { text: string; type: MessageType } | null;
  showMessage: (text: string, type?: MessageType) => void;
  clearMessage: () => void;
}

export const useMessageStore = create<MessageState>((set) => ({
  message: null,
  showMessage: (text: string, type: MessageType = 'info') => {
    set({ message: { text, type } });
  },
  clearMessage: () => {
    set({ message: null });
  },
}));
