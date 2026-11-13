import i18n from '@/i18n';
import type { ClipboardContentType } from '@/types/api';

export type DisplayKind = 'text' | 'url' | 'image' | 'file' | 'group';

export function getDisplayKind(type: ClipboardContentType, text: string): DisplayKind {
  switch (type) {
    case 'Image':
      return 'image';
    case 'File':
      return 'file';
    case 'Group':
      return 'group';
    case 'Text':
    default:
      return isURL(text) ? 'url' : 'text';
  }
}

function isURL(text: string): boolean {
  const trimmed = text.trim();
  if (trimmed.includes('\n')) return false;
  try {
    const url = new URL(trimmed);
    return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname;
  } catch {
    return false;
  }
}

export function getDisplayKindLabel(kind: DisplayKind): string {
  const labels: Record<DisplayKind, string> = {
    text: i18n.t('history:kind.text'),
    url: i18n.t('history:kind.url'),
    image: i18n.t('history:kind.image'),
    file: i18n.t('history:kind.file'),
    group: i18n.t('history:kind.group'),
  };
  return labels[kind];
}

export function getDisplayKindIcon(kind: DisplayKind): string {
  const icons: Record<DisplayKind, string> = {
    text: 'document-text',
    url: 'link',
    image: 'image',
    file: 'document',
    group: 'folder',
  };
  return icons[kind];
}

export function getDisplayKindColor(kind: DisplayKind): string {
  const colors: Record<DisplayKind, string> = {
    text: '#4285F4',
    url: '#00BCD4',
    image: '#4CAF50',
    file: '#FF9800',
    group: '#9C27B0',
  };
  return colors[kind];
}

export function getURLDomain(text: string): string {
  try {
    return new URL(text.trim()).hostname;
  } catch {
    return text.trim();
  }
}

export function getURLWithoutScheme(text: string): string {
  const trimmed = text.trim();
  const idx = trimmed.indexOf('://');
  return idx >= 0 ? trimmed.slice(idx + 3) : trimmed;
}

export function formatRelativeTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;
  if (diff < 5000) return i18n.t('history:time.justNow');
  if (diff < 60000) return i18n.t('history:time.secondsAgo', { n: Math.floor(diff / 1000) });
  if (diff < 3600000) return i18n.t('history:time.minutesAgo', { n: Math.floor(diff / 60000) });
  if (diff < 86400000) return i18n.t('history:time.hoursAgo', { n: Math.floor(diff / 3600000) });
  if (diff < 604800000) return i18n.t('history:time.daysAgo', { n: Math.floor(diff / 86400000) });
  const d = new Date(timestamp);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}
