/**
 * Utility Functions
 */

import { ClipboardContentType } from '../types/api';

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
    Text: '文本',
    Image: '图片',
    File: '文件',
    Group: '文件组',
  };
  return typeNames[type] || '未知';
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

// Export hash utilities
export * from './hash';

// Export clipboard utilities
export * from './clipboard';

// Export file storage utilities
export * from './fileStorage';

// Export config utilities
export * from './config';

// Export connect URI parser
export * from './connectUri';
