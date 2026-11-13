import { create } from "zustand";
import { Image } from "react-native";
import type { ClipboardItem } from "@/types/clipboard";
import { HistorySyncStatus } from "@/types/clipboard";

const timestamp = Date.now();
const rows: Pick<
  ClipboardItem,
  "type" | "text" | "dataName" | "size" | "fileUri"
>[] = [
  {
    type: "Text",
    text: "欢迎使用 UniClipboard。\n这是独立的 Expo UI 壳。所有记录都是演示数据。",
  },
  { type: "Text", text: "https://uniclipboard.app" },
  {
    type: "Image",
    text: "UniClipboard.png",
    dataName: "UniClipboard.png",
    size: 128000,
    fileUri: Image.resolveAssetSource(require("../../assets/icon.png")).uri,
  },
  {
    type: "File",
    text: "Design notes.pdf",
    dataName: "Design notes.pdf",
    size: 1024000,
  },
  {
    type: "Group",
    text: "Project assets",
    dataName: "Project assets",
    size: 3456000,
  },
  {
    type: "Text",
    text: "Meeting notes\n• Review mobile navigation\n• Check dark appearance\n• Verify search and selection",
  },
  {
    type: "Text",
    text: 'const clipboard = "跨设备剪贴板";\nconsole.log(clipboard);',
  },
  { type: "Text", text: "https://github.com/UniClipboard/UniClipboard" },
];

export function createDemoHistory(): ClipboardItem[] {
  return rows.map((row, index) => ({
    ...row,
    profileHash: `demo-${index + 1}`,
    hasData: row.type !== "Text",
    timestamp: timestamp - index * 3600000,
    lastModified: timestamp - index * 3600000,
    lastAccessed: timestamp - index * 3600000,
    version: 1,
    starred: index === 0,
    pinned: false,
    isDeleted: false,
    isLocalFileReady: !!row.fileUri || row.type === "Text",
    syncStatus: HistorySyncStatus.LocalOnly,
    deviceName: index % 2 ? "Demo Mac" : "Demo Phone",
    from: index % 2 ? "server" : undefined,
  }));
}

type ShellState = {
  records: ClipboardItem[];
  welcomeCompleted: boolean;
  autoApplyRemote: boolean;
  autoPushLocal: boolean;
  language: string;
  maxHistoryItems: number;
  setPreference: (
    patch: Partial<
      Pick<
        ShellState,
        | "welcomeCompleted"
        | "autoApplyRemote"
        | "autoPushLocal"
        | "language"
        | "maxHistoryItems"
      >
    >,
  ) => void;
  deleteRecords: (ids: readonly string[]) => void;
  toggleStar: (id: string) => void;
  resetHistory: () => void;
};

export const useShellStore = create<ShellState>((set) => ({
  records: createDemoHistory(),
  welcomeCompleted: true,
  autoApplyRemote: false,
  autoPushLocal: false,
  language: "system",
  maxHistoryItems: 100,
  setPreference: (patch) => set(patch),
  deleteRecords: (ids) =>
    set((state) => ({
      records: state.records.filter((item) => !ids.includes(item.profileHash)),
    })),
  toggleStar: (id) =>
    set((state) => ({
      records: state.records.map((item) =>
        item.profileHash === id ? { ...item, starred: !item.starred } : item,
      ),
    })),
  resetHistory: () => set({ records: createDemoHistory() }),
}));
