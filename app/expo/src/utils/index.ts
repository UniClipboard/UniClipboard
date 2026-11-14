/**
 * Utility Functions
 */

import { ClipboardContentType } from '../types/api';
import i18n from '@/i18n';

// Export text utilities
export * from './textUtils';

/**
 * Format timestamp to readable date string
 */
export const formatDate = (timestamp: number): string => {
  const date = new Date(timestamp);
  return date.toLocaleString('zh-CN');
};

/**
 * Get clipboard type display name
 */
export const getClipboardTypeName = (type: ClipboardContentType): string => {
  const typeNames = {
    Text: i18n.t('errors:contentType.text'),
    Image: i18n.t('errors:contentType.image'),
    File: i18n.t('errors:contentType.file'),
    Group: i18n.t('errors:contentType.group'),
  };
  return typeNames[type] || i18n.t('errors:contentType.unknown');
};

/**
 * Format file size to human readable string
 */
export const formatFileSize = (bytes: number, decimals: number = 1): string => {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(decimals)} ${sizes[i]}`;
};

/**
 * Format size with type awareness
 * Text type shows character count with locale formatting
 * Other types show file size
 */
export const formatSizeWithType = (bytes?: number, type?: string): string => {
  if (!bytes) return '';
  if (type === 'Text') {
    return bytes.toLocaleString('zh-CN');
  }
  return formatFileSize(bytes);
};

/**
 * Truncate text to specified length
 */
export const truncateText = (text: string, maxLength: number = 100): string => {
  if (text.length <= maxLength) return text;
  return text.substring(0, maxLength) + '...';
};

/**
 * Validate URL format
 */
export const isValidUrl = (url: string): boolean => {
  try {
    new URL(url);
    return true;
  } catch {
    return false;
  }
};
