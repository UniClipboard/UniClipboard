/**
 * Sync Manager Types
 * 同步管理器相关类型定义
 */

import { ServerConfig } from './api';
import { ClipboardContent } from './clipboard';

/**
 * 同步方向
 */
export enum SyncDirection {
  /** 上传 */
  Upload = 'upload',
  /** 下载 */
  Download = 'download',
  /** 双向同步 */
  Both = 'both',
}

/**
 * 同步状态
 */
export enum SyncStatus {
  /** 空闲 */
  Idle = 'idle',
  /** 同步中 */
  Syncing = 'syncing',
  /** 成功 */
  Success = 'success',
  /** 失败 */
  Failed = 'failed',
  /** 冲突 */
  Conflict = 'conflict',
}

/**
 * 同步模式
 */
export enum SyncMode {
  /** 手动同步 */
  Manual = 'manual',
  /** 自动同步 */
  Auto = 'auto',
  /** 实时同步 */
  Realtime = 'realtime',
}

/**
 * 冲突解决策略
 */
export enum ConflictResolution {
  /** 使用本地版本 */
  UseLocal = 'local',
  /** 使用远程版本 */
  UseRemote = 'remote',
  /** 使用最新版本（基于时间戳） */
  UseNewest = 'newest',
  /** 询问用户 */
  Ask = 'ask',
}

/**
 * 同步配置
 */
export interface SyncConfig {
  /** 服务器配置 */
  server: ServerConfig;

  /** 同步模式 */
  mode: SyncMode;

  /** 同步间隔（毫秒）- 仅自动模式 */
  interval?: number;

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

  /** 最大重试次数 */
  maxRetries: number;

  /** 重试延迟（毫秒） */
  retryDelay: number;
}

/**
 * 同步任务
 */
export interface SyncTask {
  /** 任务ID */
  id: string;

  /** 同步方向 */
  direction: SyncDirection;

  /** 剪贴板内容 */
  content: ClipboardContent;

  /** 创建时间 */
  createdAt: number;

  /** 重试次数 */
  retries: number;

  /** 最后错误信息 */
  lastError?: string;
}

/**
 * 同步结果
 */
export interface SyncResult {
  /** 是否成功 */
  success: boolean;

  /** 同步方向 */
  direction: SyncDirection;

  /** 错误信息 */
  error?: string;

  /** 同步的 profile hash */
  profileHash?: string;

  /** 是否跳过（内容未变化） */
  skipped?: boolean;

  /** 是否有冲突 */
  hasConflict?: boolean;

  /** 本次同步的剪贴板内容（仅下载成功且有实际内容时填充） */
  content?: ClipboardContent;

  /** 同步耗时（毫秒） */
  duration?: number;
}

/**
 * 同步事件类型
 */
export enum SyncEventType {
  /** 开始同步 */
  Started = 'started',
  /** 同步进度 */
  Progress = 'progress',
  /** 同步完成 */
  Completed = 'completed',
  /** 同步失败 */
  Failed = 'failed',
  /** 发现冲突 */
  Conflict = 'conflict',
  /** 状态变化 */
  StatusChanged = 'statusChanged',
}

/**
 * 同步事件
 */
export interface SyncEvent {
  /** 事件类型 */
  type: SyncEventType;

  /** 同步结果 */
  result?: SyncResult;

  /** 同步状态 */
  status?: SyncStatus;

  /** 额外数据 */
  data?: unknown;

  /** 时间戳 */
  timestamp: number;
}

/**
 * 同步监听器回调
 */
export type SyncListener = (event: SyncEvent) => void;

/**
 * 同步统计
 */
export interface SyncStats {
  /** 总同步次数 */
  totalSyncs: number;

  /** 成功次数 */
  successCount: number;

  /** 失败次数 */
  failureCount: number;

  /** 上传次数 */
  uploadCount: number;

  /** 下载次数 */
  downloadCount: number;

  /** 跳过次数 */
  skipCount: number;

  /** 冲突次数 */
  conflictCount: number;

  /** 最后同步时间 */
  lastSyncTime?: number;

  /** 最后成功同步时间 */
  lastSuccessTime?: number;

  /** 平均同步耗时（毫秒） */
  averageDuration?: number;
}

/**
 * 离线队列项
 */
export interface OfflineQueueItem {
  /** 任务ID */
  taskId: string;

  /** 同步任务 */
  task: SyncTask;

  /** 添加到队列的时间 */
  queuedAt: number;
}
