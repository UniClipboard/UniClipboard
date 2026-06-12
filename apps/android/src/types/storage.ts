/**
 * Storage Types
 * 本地存储相关类型定义
 */

import { ServerConfig } from './api';
import { SyncMode, ConflictResolution } from './sync';
import { HistorySyncStatus } from './clipboard';

/**
 * 应用配置
 */
export interface AppConfig {
  /** 服务器配置列表 */
  servers: ServerConfig[];

  /** 当前激活的服务器索引 */
  activeServerIndex: number;

  /** 同步模式 */
  syncMode: SyncMode;

  /** 同步间隔（毫秒） */
  syncInterval: number;

  /** 冲突解决策略 */
  conflictResolution: ConflictResolution;

  /** 是否启用离线队列 */
  enableOfflineQueue: boolean;

  /** 最大离线队列大小 */
  maxOfflineQueueSize: number;

  /** 是否同步大文件 */
  syncLargeFiles: boolean;

  /** 大文件阈值（字节） */
  largeFileThreshold: number;

  /** 主题模式 */
  theme: 'light' | 'dark' | 'auto';

  /** 语言 */
  language: string;

  /** 是否启用通知 */
  enableNotifications: boolean;

  /** 是否在后台同步 */
  syncInBackground: boolean;

  /** 启动时自动同步 */
  syncOnStartup: boolean;

  /** 自动同步（检测到变化时自动上传/下载） */
  autoSync: boolean;

  /** 自动下载最大文件大小（字节），默认 5MB */
  autoDownloadMaxSize: number;

  /** 调试模式 */
  debugMode: boolean;

  /** 历史记录最大保留条数，默认 1000 */
  maxHistoryItems: number;

  /** 是否自动检查更新（每天一次） */
  autoCheckUpdate: boolean;

  /** 上次检查更新的日期字符串（YYYY-MM-DD） */
  lastUpdateCheckDate: string;

  /** 是否更新到测试版（beta） */
  updateToBeta: boolean;

  /** 是否启用历史记录同步 */
  enableHistorySync: boolean;

  /** 是否需要整理历史记录（关闭同步或切换服务器后设置） */
  needsHistoryReorganize?: boolean;

  /** 日志等级 */
  logLevel: 'debug' | 'info' | 'warn' | 'error';

  /** 远程轮询间隔（毫秒），默认 3000ms */
  remotePollingInterval: number;

  /** 本地轮询间隔（毫秒），默认 1000ms */
  localPollingInterval: number;

  /** 后台任务总开关 */
  enableBackgroundTasks: boolean;

  /** 是否在后台时下载远程剪贴板（保持远程轮询或 SignalR 连接） */
  enableBackgroundDownload: boolean;

  /** 是否在后台时上传本地剪贴板（保持本地剪贴板监听） */
  enableBackgroundUpload: boolean;

  /** 是否在后台时使用悬浮窗获取剪贴板（仅 Android） */
  enableClipboardOverlay: boolean;

  /** 是否使用 Shizuku 在后台获取剪贴板（仅 Android） */
  enableShizukuClipboard: boolean;

  /** 是否启用自动上传短信验证码（仅 Android） */
  enableSmsForwarding: boolean;

  /** 调试时显示悬浮窗（仅 Android，调试模式下可用） */
  debugOverlayVisible: boolean;

  /** 调试时显示 URL Scheme 调用（Toast 通知） */
  debugUrlScheme: boolean;

  /** 调试时跳过更新检查每日一次的限制，每次启动都检查 */
  debugUpdateCheckNoLimit: boolean;

  /** 启用前台服务常驻通知 */
  enableForegroundNotification: boolean;

  /** 同步后发送 Toast 通知（上传/下载完成时） */
  syncToastEnabled: boolean;

  /** 在最近任务列表中隐藏应用（仅 Android） */
  hideFromRecents: boolean;

  /** 历史记录图片自动下载策略 */
  historyImageAutoDownload: 'wifi' | 'always' | 'off';

  /** 是否在历史记录的图片项显示复制按钮 */
  showImageCopyButton: boolean;
}

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

  /** 起始时间 */
  startDate?: number;

  /** 结束时间 */
  endDate?: number;

  /** 搜索关键词 */
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

/**
 * 默认应用配置
 */
export const DEFAULT_APP_CONFIG: AppConfig = {
  servers: [],
  activeServerIndex: -1,
  syncMode: 'manual' as SyncMode,
  syncInterval: 5000,
  conflictResolution: 'newest' as ConflictResolution,
  enableOfflineQueue: true,
  maxOfflineQueueSize: 100,
  syncLargeFiles: true,
  largeFileThreshold: 10 * 1024 * 1024, // 10MB
  theme: 'auto',
  language: 'zh-CN',
  enableNotifications: true,
  syncInBackground: false,
  syncOnStartup: true,
  autoSync: false,
  autoDownloadMaxSize: 5 * 1024 * 1024, // 5MB
  debugMode: false,
  maxHistoryItems: 1000, // 默认 1000 条
  autoCheckUpdate: true,
  lastUpdateCheckDate: '',
  updateToBeta: false,
  enableHistorySync: false, // 默认关闭历史记录同步
  logLevel: 'info', // 默认 info 级别
  remotePollingInterval: 3000, // 默认 3 秒
  localPollingInterval: 1000, // 默认 1 秒
  enableBackgroundTasks: false, // 默认关闭后台任务总开关
  enableBackgroundDownload: false, // 默认关闭后台下载远程
  enableBackgroundUpload: false, // 默认关闭后台上传本地
  enableClipboardOverlay: false, // 默认关闭悬浮窗获取剪贴板
  enableShizukuClipboard: false, // 默认关闭 Shizuku 获取剪贴板
  enableSmsForwarding: false, // 默认关闭自动上传短信验证码
  debugOverlayVisible: false, // 默认不显示悬浮窗
  debugUrlScheme: false, // 默认不显示 URL Scheme 调用
  debugUpdateCheckNoLimit: false, // 默认不跳过每日更新检查限制
  enableForegroundNotification: true, // 默认启用前台服务常驻通知
  syncToastEnabled: true, // 默认开启同步 Toast 通知
  hideFromRecents: false, // 默认不隐藏最近任务
  historyImageAutoDownload: 'wifi', // 默认 Wi-Fi 网络自动下载
  showImageCopyButton: false, // 默认不显示图片复制按钮
};

/**
 * 默认缓存配置
 */
export const DEFAULT_CACHE_CONFIG: CacheConfig = {
  defaultTTL: 30 * 60 * 1000, // 30分钟
  maxSize: 100,
  cleanupInterval: 5 * 60 * 1000, // 5分钟
};
