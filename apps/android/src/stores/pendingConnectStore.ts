/**
 * 一次性凭据 drop-box
 *
 * 用途：扫码 / 深链解析出的 SyncClipboard 接入凭据在被 UI 消费前的中转。
 * 通过 store 传递而不是 navigation params，避免明文密码进入 nav state / dev tools / crash report。
 *
 * 使用约定：
 * - 写入方（QrScannerModal、useConnectDeepLink）：`set({ url, user, pwd, label? })`
 * - 消费方（SettingsScreen）：`consume()` 取值并立即清空 store
 * - 永远不要把这个 store 的内容序列化到磁盘或日志
 */
import { create } from 'zustand';

export interface PendingConnectIntent {
  url: string;
  user: string;
  pwd: string;
  label?: string;
}

interface PendingConnectState {
  intent: PendingConnectIntent | null;
  set: (intent: PendingConnectIntent) => void;
  consume: () => PendingConnectIntent | null;
  clear: () => void;
}

export const usePendingConnectStore = create<PendingConnectState>((set, get) => ({
  intent: null,
  set: (intent) => set({ intent }),
  consume: () => {
    const v = get().intent;
    if (v !== null) set({ intent: null });
    return v;
  },
  clear: () => set({ intent: null }),
}));
