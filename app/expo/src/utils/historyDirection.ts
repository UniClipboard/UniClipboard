import { ClipboardItem, HistorySyncStatus } from '@/types/clipboard';

export type HistoryDirectionIndicator = 'upload' | 'download' | 'pendingUpload' | 'pendingSync';

export function getHistoryDirectionIndicator(item: ClipboardItem): HistoryDirectionIndicator {
  if (item.from || item.isLocalFileReady === false) {
    return 'download';
  }

  if (item.syncStatus === HistorySyncStatus.LocalOnly) {
    return 'pendingUpload';
  }

  if (item.syncStatus === HistorySyncStatus.NeedSync) {
    return 'pendingSync';
  }

  return 'upload';
}
