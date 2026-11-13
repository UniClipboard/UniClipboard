/**
 * Storage Types
 * 本地存储相关类型定义
 */

import { HistorySyncStatus } from './clipboard';
import type { DisplayKind } from '@/utils/displayKind';



/**
 * 缓存项
 */
export interface CacheItem<T = unknown> {
  /** 缓存键 */
  key: string;

  /** 缓存值 */
  value: T;

  /** 创建时间戳 */
  createdAt: number;

  /** 过期时间戳（可选） */
  expiresAt?: number;

  /** 访问次数 */
  accessCount: number;

  /** 最后访问时间 */
  lastAccessedAt: number;
}

/**
 * 缓存配置
 */
export interface CacheConfig {
  /** 默认过期时间（毫秒） */
  defaultTTL: number;

  /** 最大缓存条目数 */
  maxSize: number;

  /** 清理间隔（毫秒） */
  cleanupInterval: number;
}

/**
 * 历史记录过滤器
 */
export interface HistoryFilter {
  /** 内容类型 */
  type?: string[];

  /** 展示类型，文本与链接等客户端识别类型也可单独筛选 */
  displayKinds?: DisplayKind[];

  /** 起始时间(含边界,0 也是有效边界) */
  startDate?: number;

  /** 结束时间(含边界,0 也是有效边界) */
  endDate?: number;

  /** 字面子串;仅 ASCII 大小写不敏感,非 ASCII 保留大小写;空串不筛选 */
  keyword?: string;

  /** 是否仅显示标记项 */
  starredOnly?: boolean;

  /** 是否仅显示已同步项 */
  syncedOnly?: boolean;

  /** 是否仅显示置顶项 */
  pinnedOnly?: boolean;

  /** 是否仅显示本地有数据的项 */
  localOnly?: boolean;

  /** 同步状态筛选 */
  syncStatus?: HistorySyncStatus[];

  /** 是否仅显示传输中的项 */
  transferringOnly?: boolean;

  /** 来源:local = 本机产生(from 不是 'server'),remote = 从其他设备同步而来(from = 'server') */
  source?: 'local' | 'remote';
}

/**
 * 历史记录排序
 */
export interface HistorySort {
  /** 排序字段 */
  field: 'timestamp' | 'useCount' | 'size' | 'lastAccessed';

  /** 排序方向 */
  order: 'asc' | 'desc';
}

/**
 * 存储统计信息
 */
export interface StorageStats {
  /** 配置大小（字节） */
  configSize: number;

  /** 缓存大小（字节） */
  cacheSize: number;

  /** 历史记录大小（字节） */
  historySize: number;

  /** 总大小（字节） */
  totalSize: number;

  /** 历史记录数量 */
  historyCount: number;

  /** 缓存条目数量 */
  cacheCount: number;

  /** 最后更新时间 */
  lastUpdated: number;
}

/**
 * 存储键常量
 */
export const STORAGE_KEYS = {
  /** 应用配置 */
  CONFIG: '@syncclipboard:config',

  /** 服务器列表 */
  SERVERS: '@syncclipboard:servers',

  /** 历史记录 */
  HISTORY: '@syncclipboard:history',

  /** 历史记录数据版本号 */
  HISTORY_VERSION: '@syncclipboard:history:version',

  /** 缓存前缀 */
  CACHE_PREFIX: '@syncclipboard:cache:',

  /** 同步状态 */
  SYNC_STATE: '@syncclipboard:sync:state',

  /** 统计信息 */
  STATS: '@syncclipboard:stats',

  /** 上次同步时间 */
  LAST_SYNC: '@syncclipboard:last_sync',
} as const;

// DEFAULT_APP_CONFIG is now re-exported from './settings' as DEFAULT_SETTINGS at the top of this file.

/**
 * 默认缓存配置
 */
export const DEFAULT_CACHE_CONFIG: CacheConfig = {
  defaultTTL: 30 * 60 * 1000, // 30分钟
  maxSize: 100,
  cleanupInterval: 5 * 60 * 1000, // 5分钟
};
