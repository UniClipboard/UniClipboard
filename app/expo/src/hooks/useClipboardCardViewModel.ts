import { useMemo } from 'react';
import type { ClipboardItem } from '@/types/clipboard';
import {
  getDisplayKind,
  getDisplayKindLabel,
  formatRelativeTime,
  type DisplayKind,
} from '@/utils/displayKind';
import {
  getHistoryDirectionIndicator,
  type HistoryDirectionIndicator,
} from '@/utils/historyDirection';

export interface ClipboardCardViewModel {
  displayKind: DisplayKind;
  kindLabel: string;
  relativeTime: string;
  directionIndicator: HistoryDirectionIndicator;
}

/**
 * 卡片内容分支所需的平台无关派生数据，被卡片本体与 iOS 长按预览共用，
 * 避免两处各自重新判定 displayKind。
 */
export function useClipboardCardViewModel(item: ClipboardItem): ClipboardCardViewModel {
  const displayKind = useMemo(() => getDisplayKind(item.type, item.text), [item.type, item.text]);
  const kindLabel = useMemo(() => getDisplayKindLabel(displayKind), [displayKind]);
  const relativeTime = formatRelativeTime(item.timestamp);
  const directionIndicator = useMemo(() => getHistoryDirectionIndicator(item), [item]);

  return { displayKind, kindLabel, relativeTime, directionIndicator };
}
