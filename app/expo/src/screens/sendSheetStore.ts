import { create } from "zustand";
import type { ClipboardItem } from "@/types/clipboard";

type SendSheetRequest = { item: ClipboardItem; draftText?: never } | { item?: never; draftText: string };

export const useSendSheetStore = create<{
  queue: SendSheetRequest[];
  open: (request: SendSheetRequest) => void;
  finish: () => void;
}>((set) => ({
  queue: [],
  open: (request) => set((state) => {
    const alreadyQueued = state.queue.some((entry) => request.item
      ? entry.item?.profileHash === request.item.profileHash
      : entry.draftText === request.draftText);
    return alreadyQueued ? state : { queue: [...state.queue, request] };
  }),
  finish: () => set(({ queue }) => ({ queue: queue.slice(1) })),
}));
