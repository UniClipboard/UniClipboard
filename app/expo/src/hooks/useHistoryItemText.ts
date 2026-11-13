import type { ClipboardItem } from '@/types/clipboard';
export function useHistoryItemText(item: ClipboardItem): string { return item.text; }
