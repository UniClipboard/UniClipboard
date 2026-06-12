/**
 * Sync Manager
 * 同步管理器 - 管理剪贴板内容的上传和下载
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { ToastAndroid, Platform } from 'react-native';
import { SyncClipboardClient } from './SyncClipboardClient';
import { ISyncClipboardAPI } from './APIClient';
import { WebDAVClient } from './WebDAVClient';
import { S3Client } from './S3Client';
import { AuthService } from './AuthService';
import { clipboardManager } from './ClipboardManager';
import { clipboardMonitor } from './ClipboardMonitor';
import { ConfigurationError } from './errors';
import { ServerConfig, ProfileDto } from '../types/api';
import { compareHash } from '../utils/hash';
import { isTextInvalid } from '../utils/index';
import type { ProgressInfo } from 'native-util';
import {
  SyncConfig,
  SyncStatus,
  SyncMode,
  SyncDirection,
  SyncResult,
  SyncTask,
  SyncEvent,
  SyncEventType,
  SyncListener,
  SyncStats,
  ConflictResolution,
  OfflineQueueItem,
} from '../types/sync';
import { ClipboardContent } from '../types/clipboard';
import { useSettingsStore } from '../stores/settingsStore';

const STORAGE_KEY_CONFIG = '@syncclipboard:sync:config';
const STORAGE_KEY_STATS = '@syncclipboard:sync:stats';
const STORAGE_KEY_QUEUE = '@syncclipboard:sync:queue';
const STORAGE_KEY_LAST_PROFILE_HASH = '@syncclipboard:sync:last_hash';

/**
 * 扩展的错误接口，包含网络错误标志
 */
interface ExtendedError extends Error {
  isNetworkError?: boolean;
}

/**
 * 默认同步配置
 */
const DEFAULT_CONFIG: Partial<SyncConfig> = {
  mode: SyncMode.Manual,
  interval: 5000, // 5秒
  conflictResolution: ConflictResolution.UseNewest,
  enableOfflineQueue: true,
  maxOfflineQueueSize: 100,
  syncLargeFiles: true,
  largeFileThreshold: 10 * 1024 * 1024, // 10MB
  maxRetries: 3,
  retryDelay: 2000, // 2秒
};

/**
 * 同步管理器
 */
export class SyncManager {
  private static instance: SyncManager | null = null;

  private config: SyncConfig | null = null;
  private apiClient: ISyncClipboardAPI | null = null;
  private clipboardManager = clipboardManager;
  private clipboardMonitor = clipboardMonitor;

  private status: SyncStatus = SyncStatus.Idle;
  private listeners: Map<string, SyncListener> = new Map();
  private stats: SyncStats = {
    totalSyncs: 0,
    successCount: 0,
    failureCount: 0,
    uploadCount: 0,
    downloadCount: 0,
    skipCount: 0,
    conflictCount: 0,
  };

  private syncTimer: NodeJS.Timeout | null = null;
  private isSyncing = false;
  private currentSyncPromise: Promise<SyncResult> | null = null;
  private currentSyncAbortController: AbortController | null = null;
  private lastLocalProfileHash: string | null = null;
  private lastRemoteProfileHash: string | null = null;
  private offlineQueue: OfflineQueueItem[] = [];
  private realtimeSyncCallback: ((content: ClipboardContent) => Promise<void>) | null = null;
  private pendingUploadContent: ClipboardContent | null = null;

  private constructor() {
    // Singleton instances are initialized as class properties
  }

  /**
   * 获取单例实例
   */
  public static getInstance(): SyncManager {
    if (!SyncManager.instance) {
      SyncManager.instance = new SyncManager();
    }
    return SyncManager.instance;
  }

  /**
   * 获取当前 API 客户端（供外部服务使用）
   */
  public getAPIClient(): ISyncClipboardAPI | null {
    return this.apiClient;
  }

  public setPendingUploadContent(content: ClipboardContent | null): void {
    this.pendingUploadContent = content;
  }

  /**
   * 获取最后上传的 profile hash（用于外部去重判断）
   */
  public getLastUploadedHash(): string | null {
    return this.lastLocalProfileHash;
  }

  /**
   * 设置最后上传的 profile hash（供绕过 SyncManager.sync() 的直接上传路径设置，避免触发自动下载）
   */
  public setLastUploadedHash(hash: string): void {
    this.lastLocalProfileHash = hash;
  }

  /**
   * 更新前台服务通知文本（自动附加时间戳）
   */
  public updateForegroundNotification(text: string): void {
    if (Platform.OS !== 'android') return;
    // 将 "已上传: preview" 格式转为 "↑ 已上传\npreview"
    const colonIdx = text.indexOf(': ');
    let content: string;
    if (colonIdx >= 0) {
      const action = text.slice(0, colonIdx);
      const preview = text.slice(colonIdx + 2);
      content = `${action}\n${preview}`;
    } else {
      content = `SyncClipboard\n${text}`;
    }
    import('foreground-service')
      .then((ForegroundService) => {
        ForegroundService.updateNotification(content);
      })
      .catch(() => {
        // foreground service module not available
      });
  }

  /**
   * 创建 API 客户端
   */
  private createAPIClient(config: ServerConfig): ISyncClipboardAPI {
    const { type, url, username, password } = config;

    if (type === 'syncclipboard') {
      if (!url) {
        throw new ConfigurationError('Server URL is required');
      }
      const authService = username && password ? new AuthService(username, password) : undefined;
      return new SyncClipboardClient({ baseURL: url, authService });
    }

    if (type === 's3') {
      if (!config.bucketName) {
        throw new ConfigurationError('Bucket name is required for S3');
      }
      if (!username || !password) {
        throw new ConfigurationError('Access Key ID and Secret Access Key are required for S3');
      }
      return new S3Client({
        serviceURL: url || undefined,
        region: config.region,
        bucketName: config.bucketName,
        objectPrefix: config.objectPrefix,
        forcePathStyle: config.forcePathStyle,
        accessKeyId: username,
        secretAccessKey: password,
      });
    }

    // 非 SyncClipboard/S3 服务器，使用 WebDAV 客户端
    if (!url) {
      throw new ConfigurationError('Server URL is required');
    }
    if (!username || !password) {
      throw new ConfigurationError('Username and password are required for WebDAV');
    }
    return new WebDAVClient({ baseURL: url, username, password });
  }

  /**
   * 初始化同步管理器
   */
  public async initialize(config: SyncConfig): Promise<void> {
    this.config = { ...DEFAULT_CONFIG, ...config } as SyncConfig;

    // 创建 API 客户端
    this.apiClient = this.createAPIClient(config.server);

    // 加载持久化数据
    await this.loadPersistedData();

    // 如果是自动模式，启动自动同步
    if (this.config.mode === SyncMode.Auto) {
      this.startAutoSync();
    }

    // 始终监听剪贴板变化以支持后台自动上传
    // （React useEffect 在后台不可靠，需要通过 ClipboardMonitor 回调直接触发）
    this.startRealtimeSync();

    // 处理离线队列
    if (this.config.enableOfflineQueue && this.offlineQueue.length > 0) {
      await this.processOfflineQueue();
    }
  }

  /**
   * 销毁同步管理器
   */
  public async destroy(): Promise<void> {
    this.stopAutoSync();
    this.stopRealtimeSync();
    await this.savePersistedData();
    this.listeners.clear();
  }

  /**
   * 手动同步
   */
  public async sync(
    direction: SyncDirection = SyncDirection.Both,
    isAuto: boolean = false,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void,
    onPreview?: (preview: string) => void
  ): Promise<SyncResult> {
    if (!this.config || !this.apiClient) {
      throw new Error('SyncManager not initialized');
    }

    if (this.isSyncing) {
      if (isAuto) {
        // 自动同步：跳过本次执行
        return {
          success: false,
          skipped: true,
          direction,
          error: 'Sync already in progress',
        };
      }
      // 手动/快速操作：取消当前同步后再执行
      if (this.currentSyncAbortController) {
        this.currentSyncAbortController.abort();
      }
      if (this.currentSyncPromise) {
        await this.currentSyncPromise.catch(() => {});
      }
    }

    // 创建内部 AbortController，与外部 signal 合并
    const internalAbortController = new AbortController();
    this.currentSyncAbortController = internalAbortController;
    let mergedSignal: AbortSignal;
    if (signal) {
      // 外部 signal 取消时也取消内部 controller
      const onExternalAbort = () => internalAbortController.abort();
      signal.addEventListener('abort', onExternalAbort, { once: true });
      mergedSignal = internalAbortController.signal;
    } else {
      mergedSignal = internalAbortController.signal;
    }

    const startTime = Date.now();
    this.isSyncing = true;
    this.setStatus(SyncStatus.Syncing);
    this.emitEvent({
      type: SyncEventType.Started,
      timestamp: Date.now(),
    });

    const doSync = async (): Promise<SyncResult> => {
      try {
        let result: SyncResult;

        switch (direction) {
          case SyncDirection.Upload:
            result = await this.upload(isAuto, mergedSignal, onProgress, onPreview);
            break;
          case SyncDirection.Download:
            result = await this.download(isAuto, mergedSignal, onProgress, onPreview);
            break;
          case SyncDirection.Both:
            // 先下载后上传，避免覆盖远程内容
            const downloadResult = await this.download(isAuto, mergedSignal, onProgress, onPreview);
            if (downloadResult.success || downloadResult.skipped) {
              const uploadResult = await this.upload(isAuto, mergedSignal, onProgress, onPreview);
              result = uploadResult;
            } else {
              result = downloadResult;
            }
            break;
        }

        result.duration = Date.now() - startTime;

        // 更新统计信息
        this.updateStats(result);

        // 发送完成事件
        this.emitEvent({
          type: SyncEventType.Completed,
          result,
          timestamp: Date.now(),
        });

        this.setStatus(result.success ? SyncStatus.Success : SyncStatus.Failed);

        return result;
      } catch (error) {
        // 用户取消操作不视为失败
        if (error instanceof Error && error.name === 'AbortError') {
          const result: SyncResult = {
            success: false,
            direction,
            error: error.message,
            duration: Date.now() - startTime,
            skipped: true,
          };

          this.setStatus(SyncStatus.Idle);

          return result;
        }

        // 提取详细错误信息，包含HTTP状态码
        let errorMessage = 'Unknown error';
        if (error instanceof Error) {
          errorMessage = error.message;
          // 如果错误对象包含statusCode属性，添加到错误消息中
          if ('statusCode' in error && typeof error.statusCode === 'number') {
            errorMessage = `HTTP ${error.statusCode}: ${errorMessage}`;
          }
        }

        const result: SyncResult = {
          success: false,
          direction,
          error: errorMessage,
          duration: Date.now() - startTime,
        };

        this.updateStats(result);
        this.emitEvent({
          type: SyncEventType.Failed,
          result,
          timestamp: Date.now(),
        });

        this.setStatus(SyncStatus.Failed);

        return result;
      } finally {
        this.isSyncing = false;
        this.currentSyncPromise = null;
        this.currentSyncAbortController = null;
        await this.savePersistedData();
      }
    };

    this.currentSyncPromise = doSync();
    return this.currentSyncPromise;
  }

  /**
   * 上传剪贴板内容
   */
  private async upload(
    isAuto: boolean = false,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void,
    onPreview?: (preview: string) => void
  ): Promise<SyncResult> {
    if (!this.apiClient || !this.config) {
      throw new Error('SyncManager not initialized');
    }

    try {
      // 优先使用已缓存的内容（来自 ClipboardMonitor 回调，避免后台时重新创建悬浮窗）
      let localContent =
        this.pendingUploadContent || (await this.clipboardManager.getClipboardContent());

      if (!localContent) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        localContent = await this.clipboardManager.getClipboardContent();
      }

      if (!localContent) {
        return {
          success: true,
          direction: SyncDirection.Upload,
          skipped: true,
        };
      }

      // 调用预览回调
      if (onPreview) {
        if (localContent.type === 'Text' && !isTextInvalid(localContent.text)) {
          const preview = localContent.text.trim().replace(/\s+/g, ' ');
          onPreview(preview.length > 40 ? preview.slice(0, 40) + '…' : preview);
        } else if (localContent.type !== 'Text' && localContent.fileName) {
          onPreview(localContent.fileName);
        }
      }

      // 计算当前 profileHash
      const currentProfileHash = localContent.profileHash;

      // 如果内容未变化，跳过上传（仅在自动同步时）
      if (
        isAuto &&
        this.lastLocalProfileHash &&
        currentProfileHash &&
        compareHash(currentProfileHash, this.lastLocalProfileHash)
      ) {
        return {
          success: true,
          direction: SyncDirection.Upload,
          profileHash: currentProfileHash,
          skipped: true,
        };
      }

      // 检查是否是大文件（仅在自动同步时）
      if (isAuto && localContent.fileSize) {
        const isLargeFile = localContent.fileSize > this.config.largeFileThreshold;
        if (isLargeFile && !this.config.syncLargeFiles) {
          return {
            success: false,
            direction: SyncDirection.Upload,
            error: `File too large (${localContent.fileSize} bytes)`,
          };
        }
      }

      // 转换为 ProfileDto
      const { contentToProfileDto } = await import('../utils/clipboard');
      const profile = await contentToProfileDto(localContent);

      console.log('[SyncManager] Upload - Profile info:', {
        type: profile.type,
        hasData: profile.hasData,
        dataName: profile.dataName,
        size: profile.size,
      });

      console.log('[SyncManager] Upload - Content info:', {
        type: localContent.type,
        hasFileData: !!localContent.fileData,

        fileUri: localContent.fileUri,
        fileSize: localContent.fileSize,
      });

      // 预设最后上传的 profileHash（防止 SignalR 在 HTTP 响应返回前推送通知导致误触自动下载）
      const previousProfileHash = this.lastLocalProfileHash;
      if (currentProfileHash) {
        this.lastLocalProfileHash = currentProfileHash;
      }

      // 使用 putContent 统一处理：先上传数据（如果有），再上传配置
      try {
        await this.apiClient.putContent(localContent, { signal, onProgress });
      } catch (uploadError) {
        // 上传失败，回滚 hash
        this.lastLocalProfileHash = previousProfileHash;
        throw uploadError;
      }

      console.log('[SyncManager] Content uploaded successfully');

      // 持久化 profileHash
      if (currentProfileHash) {
        await AsyncStorage.setItem(STORAGE_KEY_LAST_PROFILE_HASH, currentProfileHash);
      }

      return {
        success: true,
        direction: SyncDirection.Upload,
        profileHash: currentProfileHash,
        content: localContent,
      };
    } catch (error) {
      console.error('[SyncManager] Upload failed with error:', error);
      console.error('[SyncManager] Error type:', typeof error);
      console.error('[SyncManager] Error details:', {
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
        name: error instanceof Error ? error.name : undefined,
      });

      // 使用已经处理好的错误信息
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';

      // 检查是否是网络错误（已由 putContent 处理）
      const isNetworkError = (error as ExtendedError)?.isNetworkError || false;

      // 如果启用离线队列且是网络错误，添加到队列
      if (this.config.enableOfflineQueue && isNetworkError) {
        const content = await this.clipboardManager.getClipboardContent();
        if (content) {
          const task: SyncTask = {
            id: `upload-${Date.now()}`,
            direction: SyncDirection.Upload,
            content,
            createdAt: Date.now(),
            retries: 0,
          };
          await this.addToOfflineQueue(task);
        }
        return {
          success: false,
          direction: SyncDirection.Upload,
          error: `网络错误，已添加到队列: ${errorMessage}`,
        };
      } else {
        return {
          success: false,
          direction: SyncDirection.Upload,
          error: errorMessage,
        };
      }
    }
  }

  /**
   * 下载剪贴板内容
   */
  private async download(
    isAuto: boolean = false,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void,
    onPreview?: (preview: string) => void
  ): Promise<SyncResult> {
    if (!this.apiClient || !this.config) {
      throw new Error('SyncManager not initialized');
    }

    try {
      // 获取远程剪贴板配置
      const profile = await this.apiClient.getClipboard(signal);

      if (!profile || !profile.hash) {
        return {
          success: true,
          direction: SyncDirection.Download,
          skipped: true,
        };
      }

      const remoteProfileHash = profile.hash;

      // 调用预览回调
      if (onPreview) {
        if (profile.type === 'Text' && profile.hasData && !isTextInvalid(profile.text)) {
          const preview = profile.text.trim().replace(/\s+/g, ' ');
          onPreview(preview.length > 40 ? preview.slice(0, 40) + '…' : preview);
        } else if (profile.hasData && profile.dataName) {
          onPreview(profile.dataName);
        }
      }

      // 如果远程内容未变化，跳过下载（仅在自动同步时）
      if (
        isAuto &&
        this.lastRemoteProfileHash &&
        compareHash(remoteProfileHash, this.lastRemoteProfileHash)
      ) {
        return {
          success: true,
          direction: SyncDirection.Download,
          profileHash: remoteProfileHash,
          skipped: true,
        };
      }

      // 获取本地剪贴板内容（用于冲突检测）
      const localContent = await this.clipboardManager.getClipboardContent();

      // 检测冲突
      if (localContent && localContent.profileHash) {
        if (
          !compareHash(localContent.profileHash, remoteProfileHash) &&
          this.lastLocalProfileHash &&
          !compareHash(localContent.profileHash, this.lastLocalProfileHash)
        ) {
          // 本地和远程都有修改，存在冲突
          const resolution = await this.resolveConflict(localContent, profile);

          if (resolution === 'local') {
            // 使用本地版本，上传覆盖远程
            return await this.upload(isAuto, signal, onProgress, onPreview);
          } else if (resolution === 'skip') {
            // 跳过此次同步
            return {
              success: true,
              direction: SyncDirection.Download,
              profileHash: remoteProfileHash,
              hasConflict: true,
              skipped: true,
            };
          }
          // 否则继续下载（使用远程版本）
        }
      }

      // 转换为 ClipboardContent
      const { profileDtoToContent } = await import('../utils/clipboard');
      const content = profileDtoToContent(profile);

      // 如果有文件数据，优先从历史记录读取缓存，否则下载并保存到历史记录
      if (profile.hasData && profile.dataName) {
        // 检查文件大小是否超过"允许自动同步的数据大小"限制
        const autoDownloadMaxSize =
          useSettingsStore.getState().config?.autoDownloadMaxSize ?? 5 * 1024 * 1024;
        if (profile.size && profile.size > autoDownloadMaxSize) {
          console.log(
            `[SyncManager] File too large (${profile.size} bytes > ${autoDownloadMaxSize} bytes), skipping auto-download`
          );
          return {
            success: true,
            direction: SyncDirection.Download,
            profileHash: remoteProfileHash,
            skipped: true,
          };
        }
        const { downloadAndAddToHistory } = await import('../utils/remoteClipboard');
        const updatedContent = await downloadAndAddToHistory(
          content,
          this.apiClient,
          true,
          signal,
          onProgress
        );
        content.fileUri = updatedContent.fileUri;
      }

      // 设置到本地剪贴板（仅 Text 类型，图片和文件不写入系统剪贴板）
      if (content.type === 'Text') {
        await this.clipboardManager.setClipboardContent(content);
      }

      // 更新最后下载的 profileHash
      this.lastRemoteProfileHash = remoteProfileHash;

      return {
        success: true,
        direction: SyncDirection.Download,
        profileHash: remoteProfileHash,
        content,
      };
    } catch (error) {
      throw error;
    }
  }

  /**
   * 启动自动同步
   */
  private startAutoSync(): void {
    if (!this.config) return;

    this.stopAutoSync();

    const interval = this.config.interval || 5000;
    this.syncTimer = setInterval(() => {
      this.sync(SyncDirection.Both, true).catch((error) => {
        console.error('Auto sync failed:', error);
      });
    }, interval);
  }

  /**
   * 停止自动同步
   */
  private stopAutoSync(): void {
    if (this.syncTimer) {
      clearInterval(this.syncTimer);
      this.syncTimer = null;
    }
  }

  /**
   * 启动实时同步
   */
  private startRealtimeSync(): void {
    this.realtimeSyncCallback = async (content: ClipboardContent) => {
      // 检查是否启用了自动同步
      const appConfig = useSettingsStore.getState().config;
      if (!(appConfig?.autoSync ?? false)) return;
      // 保存已读取的内容，避免 upload 重新读取剪贴板（后台时第二次悬浮窗读取可能失败）
      this.pendingUploadContent = content;
      // 当剪贴板变化时，上传新内容
      const result = await this.sync(SyncDirection.Upload, true);
      this.pendingUploadContent = null;
      // 显示系统 Toast 通知
      if (result.success && !result.skipped && Platform.OS === 'android') {
        const preview = this.getContentPreview(content);
        if (appConfig?.syncToastEnabled !== false) {
          ToastAndroid.show(`已上传\n${preview}`, ToastAndroid.SHORT);
        }
        this.updateForegroundNotification(`已上传: ${preview}`);
      }
    };
    this.clipboardMonitor.addCallback(this.realtimeSyncCallback);
  }

  /**
   * 停止实时同步（只移除自己的回调，不停止整个 ClipboardMonitor）
   */
  private stopRealtimeSync(): void {
    if (this.realtimeSyncCallback) {
      this.clipboardMonitor.removeCallback(this.realtimeSyncCallback);
      this.realtimeSyncCallback = null;
    }
  }

  /**
   * 获取内容预览文本（用于 Toast 通知）
   */
  public getContentPreview(content: ClipboardContent): string {
    if (content.type === 'Text' && content.text) {
      const text = content.text.trim().replace(/\s+/g, ' ');
      return text.length > 30 ? text.slice(0, 30) + '…' : text;
    }
    if (content.fileName) {
      return content.fileName;
    }
    return content.type;
  }

  /**
   * 处理离线队列
   */
  private async processOfflineQueue(): Promise<void> {
    if (!this.config?.enableOfflineQueue || this.offlineQueue.length === 0) {
      return;
    }

    const maxRetries = this.config.maxRetries || 3;
    const failedTasks: OfflineQueueItem[] = [];

    for (const item of this.offlineQueue) {
      try {
        // 尝试执行任务
        if (item.task.direction === SyncDirection.Upload) {
          await this.upload();
        } else if (item.task.direction === SyncDirection.Download) {
          await this.download();
        }

        // 任务成功，不添加回队列
      } catch (error) {
        // 任务失败，增加重试次数
        item.task.retries++;
        item.task.lastError = error instanceof Error ? error.message : 'Unknown error';

        // 如果未达到最大重试次数，保留在队列中
        if (item.task.retries < maxRetries) {
          failedTasks.push(item);
        } else {
          console.error(`Task ${item.taskId} exceeded max retries:`, error);
        }
      }
    }

    // 更新队列（只保留失败但未超过重试次数的任务）
    this.offlineQueue = failedTasks;
    await this.saveOfflineQueue();
  }

  /**
   * 解决冲突
   */
  private async resolveConflict(
    localContent: ClipboardContent,
    remoteProfile: ProfileDto
  ): Promise<'local' | 'remote' | 'skip'> {
    if (!this.config) {
      return 'remote';
    }

    switch (this.config.conflictResolution) {
      case ConflictResolution.UseLocal:
        return 'local';

      case ConflictResolution.UseRemote:
        return 'remote';

      case ConflictResolution.UseNewest:
        // 比较时间戳（假设 remoteProfile 有时间戳）
        // 如果没有时间戳，默认使用远程版本
        return 'remote';

      case ConflictResolution.Ask:
        // 发送冲突事件，等待用户决策
        this.emitEvent({
          type: SyncEventType.Conflict,
          data: { localContent, remoteProfile },
          timestamp: Date.now(),
        });
        // 暂时跳过，等待用户手动解决
        return 'skip';

      default:
        return 'remote';
    }
  }

  /**
   * 判断是否是网络错误
   */
  private isNetworkError(error: unknown): boolean {
    if (error instanceof Error) {
      const message = error.message.toLowerCase();
      return (
        message.includes('network') ||
        message.includes('timeout') ||
        message.includes('connection') ||
        message.includes('econnrefused') ||
        message.includes('offline')
      );
    }
    return false;
  }

  /**
   * 添加任务到离线队列
   */
  private async addToOfflineQueue(task: SyncTask): Promise<void> {
    if (!this.config?.enableOfflineQueue) return;

    const item: OfflineQueueItem = {
      taskId: task.id,
      task,
      queuedAt: Date.now(),
    };

    this.offlineQueue.push(item);

    // 限制队列大小
    const maxSize = this.config.maxOfflineQueueSize || 100;
    if (this.offlineQueue.length > maxSize) {
      this.offlineQueue.shift(); // 移除最旧的任务
    }

    await this.saveOfflineQueue();
  }

  /**
   * 设置同步状态
   */
  private setStatus(status: SyncStatus): void {
    if (this.status !== status) {
      this.status = status;
      this.emitEvent({
        type: SyncEventType.StatusChanged,
        status,
        timestamp: Date.now(),
      });
    }
  }

  /**
   * 添加事件监听器
   */
  public addListener(id: string, listener: SyncListener): void {
    this.listeners.set(id, listener);
  }

  /**
   * 移除事件监听器
   */
  public removeListener(id: string): void {
    this.listeners.delete(id);
  }

  /**
   * 发送事件
   */
  private emitEvent(event: SyncEvent): void {
    this.listeners.forEach((listener) => {
      try {
        listener(event);
      } catch (error) {
        console.error('Error in sync listener:', error);
      }
    });
  }

  /**
   * 更新统计信息
   */
  private updateStats(result: SyncResult): void {
    this.stats.totalSyncs++;
    this.stats.lastSyncTime = Date.now();

    if (result.success) {
      this.stats.successCount++;
      this.stats.lastSuccessTime = Date.now();

      if (result.direction === SyncDirection.Upload) {
        this.stats.uploadCount++;
      } else if (result.direction === SyncDirection.Download) {
        this.stats.downloadCount++;
      }
    } else {
      this.stats.failureCount++;
    }

    if (result.skipped) {
      this.stats.skipCount++;
    }

    if (result.hasConflict) {
      this.stats.conflictCount++;
    }

    // 更新平均耗时
    if (result.duration) {
      const currentAvg = this.stats.averageDuration || 0;
      const totalCount = this.stats.successCount;
      this.stats.averageDuration = (currentAvg * (totalCount - 1) + result.duration) / totalCount;
    }
  }

  /**
   * 加载持久化数据
   */
  private async loadPersistedData(): Promise<void> {
    try {
      // 加载统计信息
      const statsJson = await AsyncStorage.getItem(STORAGE_KEY_STATS);
      if (statsJson) {
        this.stats = JSON.parse(statsJson);
      }

      // 加载离线队列
      const queueJson = await AsyncStorage.getItem(STORAGE_KEY_QUEUE);
      if (queueJson) {
        this.offlineQueue = JSON.parse(queueJson);
      }

      // 加载最后的 profileHash 值
      this.lastLocalProfileHash = await AsyncStorage.getItem(STORAGE_KEY_LAST_PROFILE_HASH);
    } catch (error) {
      console.error('Failed to load persisted data:', error);
    }
  }

  /**
   * 保存持久化数据
   */
  private async savePersistedData(): Promise<void> {
    try {
      await AsyncStorage.multiSet([
        [STORAGE_KEY_STATS, JSON.stringify(this.stats)],
        [STORAGE_KEY_QUEUE, JSON.stringify(this.offlineQueue)],
        [STORAGE_KEY_LAST_PROFILE_HASH, this.lastLocalProfileHash || ''],
      ]);
    } catch (error) {
      console.error('Failed to save persisted data:', error);
    }
  }

  /**
   * 保存离线队列
   */
  private async saveOfflineQueue(): Promise<void> {
    try {
      await AsyncStorage.setItem(STORAGE_KEY_QUEUE, JSON.stringify(this.offlineQueue));
    } catch (error) {
      console.error('Failed to save offline queue:', error);
    }
  }

  /**
   * 获取当前状态
   */
  public getStatus(): SyncStatus {
    return this.status;
  }

  /**
   * 获取统计信息
   */
  public getStats(): SyncStats {
    return { ...this.stats };
  }

  /**
   * 获取离线队列大小
   */
  public getOfflineQueueSize(): number {
    return this.offlineQueue.length;
  }

  /**
   * 清空离线队列
   */
  public async clearOfflineQueue(): Promise<void> {
    this.offlineQueue = [];
    await this.saveOfflineQueue();
  }

  /**
   * 更新配置
   */
  public async updateConfig(config: Partial<SyncConfig>): Promise<void> {
    if (!this.config) {
      throw new Error('SyncManager not initialized');
    }

    const oldMode = this.config.mode;
    this.config = { ...this.config, ...config };

    // 如果模式改变，重新启动同步
    if (oldMode !== this.config.mode) {
      this.stopAutoSync();
      this.stopRealtimeSync();

      if (this.config.mode === SyncMode.Auto) {
        this.startAutoSync();
      } else if (this.config.mode === SyncMode.Realtime) {
        this.startRealtimeSync();
      }
    }

    await AsyncStorage.setItem(STORAGE_KEY_CONFIG, JSON.stringify(this.config));
  }

  /**
   * 获取配置
   */
  public getConfig(): SyncConfig | null {
    return this.config ? { ...this.config } : null;
  }
}
