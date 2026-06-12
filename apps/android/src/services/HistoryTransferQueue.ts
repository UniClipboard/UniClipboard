/**
 * History Transfer Queue
 * 历史记录传输队列 - 管理数据上传/下载任务
 */

import { IHistoryAPI, RecordNotFoundError } from './HistoryAPI';
import { HistoryStorage } from './HistoryStorage';
import { HistorySyncStatus } from '@/types/clipboard';
import { getHistoryFileDir } from '@/utils/fileStorage';
import { File } from 'expo-file-system';

export type TransferType = 'upload' | 'download';
export type TransferTaskStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'waitForRetry';

export interface TransferTask {
  profileId: string;
  displayName: string;
  type: TransferType;
  status: TransferTaskStatus;
  progress: number;
  bytesTransferred: number;
  totalBytes?: number;
  createdTime: number;
  startedTime?: number;
  completedTime?: number;
  errorMessage?: string;
  failureCount: number;
  isImmediateTask: boolean;
  abortController: AbortController;
  userCancelled?: boolean;
}

export interface TransferQueueConfig {
  maxConcurrency: number;
  maxConsecutiveFailures: number;
  retryDelayMs: number;
}

export interface TaskStatusChangedCallback {
  (task: TransferTask): void;
}

const DEFAULT_CONFIG: TransferQueueConfig = {
  maxConcurrency: 3,
  maxConsecutiveFailures: 5,
  retryDelayMs: 3000,
};

export class HistoryTransferQueue {
  private historyStorage: HistoryStorage;
  private historyAPI: IHistoryAPI | null = null;

  private pendingTasks: TransferTask[] = [];
  private activeTasks: Map<string, TransferTask> = new Map();
  private taskStatusCallbacks: Set<TaskStatusChangedCallback> = new Set();

  private config: TransferQueueConfig;
  private isRunning = false;
  private consecutiveFailures = 0;
  private processQueuePromise: Promise<void> | null = null;
  private queueSignal: { resolve: () => void } | null = null;

  constructor(config?: Partial<TransferQueueConfig>) {
    this.historyStorage = HistoryStorage.getInstance();
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  /**
   * 设置 History API
   */
  setHistoryAPI(api: IHistoryAPI): void {
    this.historyAPI = api;
  }

  /**
   * 启动队列
   */
  async start(): Promise<void> {
    if (this.isRunning) return;

    this.isRunning = true;
    this.consecutiveFailures = 0;
    this.processQueuePromise = this.processQueueLoop();
  }

  /**
   * 停止队列
   */
  async stop(): Promise<void> {
    this.isRunning = false;

    // 取消所有任务
    for (const task of this.activeTasks.values()) {
      task.abortController.abort();
      task.status = 'cancelled';
      this.notifyStatusChanged(task);
    }

    // 取消待处理任务
    for (const task of this.pendingTasks) {
      task.abortController.abort();
      task.status = 'cancelled';
      this.notifyStatusChanged(task);
    }

    this.activeTasks.clear();
    this.pendingTasks = [];

    // 唤醒队列处理循环
    this.signalQueue();

    if (this.processQueuePromise) {
      await this.processQueuePromise;
      this.processQueuePromise = null;
    }
  }

  /**
   * 清空队列
   */
  clearQueue(): void {
    console.log('[HistoryTransferQueue] Clearing queue');

    // 取消所有活动任务
    for (const task of this.activeTasks.values()) {
      try {
        task.abortController.abort();
        task.status = 'cancelled';
        this.notifyStatusChanged(task);
      } catch (error) {
        console.error('[HistoryTransferQueue] Error cancelling task:', error);
      }
    }

    // 清空待处理队列
    for (const task of this.pendingTasks) {
      task.abortController.abort();
      task.status = 'cancelled';
      this.notifyStatusChanged(task);
    }

    this.pendingTasks = [];
    this.activeTasks.clear();
  }

  /**
   * 添加下载任务
   */
  async addDownloadTask(profileId: string, isImmediate: boolean = false): Promise<TransferTask> {
    const existingTask = this.findTask(profileId, 'download');
    if (existingTask) {
      if (isImmediate && !existingTask.isImmediateTask) {
        existingTask.isImmediateTask = true;
      }
      return existingTask;
    }

    const { parseProfileId } = await import('./HistoryAPI');
    const parsed = parseProfileId(profileId);
    const item = parsed ? await this.historyStorage.getItem(parsed.hash) : null;
    const displayName = item?.text || profileId;

    const task: TransferTask = {
      profileId,
      displayName,
      type: 'download',
      status: 'pending',
      progress: -1,
      bytesTransferred: 0,
      createdTime: Date.now(),
      failureCount: 0,
      isImmediateTask: isImmediate,
      abortController: new AbortController(),
    };

    this.pendingTasks.push(task);
    this.notifyStatusChanged(task);
    this.signalQueue();

    console.log(`[HistoryTransferQueue] Added download task: ${profileId}`);
    return task;
  }

  /**
   * 添加上传任务
   */
  async addUploadTask(profileId: string, isImmediate: boolean = false): Promise<TransferTask> {
    const existingTask = this.findTask(profileId, 'upload');
    if (existingTask) {
      if (isImmediate && !existingTask.isImmediateTask) {
        existingTask.isImmediateTask = true;
      }
      return existingTask;
    }

    const { parseProfileId } = await import('./HistoryAPI');
    const parsed = parseProfileId(profileId);
    const item = parsed ? await this.historyStorage.getItem(parsed.hash) : null;
    const displayName = item?.text || profileId;

    const task: TransferTask = {
      profileId,
      displayName,
      type: 'upload',
      status: 'pending',
      progress: -1,
      bytesTransferred: 0,
      createdTime: Date.now(),
      failureCount: 0,
      isImmediateTask: isImmediate,
      abortController: new AbortController(),
    };

    this.pendingTasks.push(task);
    this.notifyStatusChanged(task);
    this.signalQueue();

    console.log(`[HistoryTransferQueue] Added upload task: ${profileId}`);
    return task;
  }

  /**
   * 取消任务
   */
  cancelTask(profileId: string, type: TransferType): boolean {
    const task = this.findTask(profileId, type);
    if (!task) return false;

    task.abortController.abort();
    task.status = 'cancelled';
    task.userCancelled = true;
    this.notifyStatusChanged(task);

    // 从待处理队列移除
    const pendingIndex = this.pendingTasks.findIndex(
      (t) => t.profileId === profileId && t.type === type
    );
    if (pendingIndex >= 0) {
      this.pendingTasks.splice(pendingIndex, 1);
    }

    // 从活动任务移除
    const key = this.getTaskKey(profileId, type);
    this.activeTasks.delete(key);

    return true;
  }

  /**
   * 获取所有活动任务
   */
  getActiveTasks(): TransferTask[] {
    return Array.from(this.activeTasks.values());
  }

  /**
   * 获取待处理任务数量
   */
  getPendingCount(): number {
    return this.pendingTasks.length;
  }

  /**
   * 获取活动任务数量
   */
  getActiveCount(): number {
    return this.activeTasks.size;
  }

  /**
   * 添加状态变化回调
   */
  onTaskStatusChanged(callback: TaskStatusChangedCallback): void {
    this.taskStatusCallbacks.add(callback);
  }

  /**
   * 移除状态变化回调
   */
  offTaskStatusChanged(callback: TaskStatusChangedCallback): void {
    this.taskStatusCallbacks.delete(callback);
  }

  /**
   * 等待所有任务完成
   */
  async waitAllTasks(signal?: AbortSignal): Promise<void> {
    while ((this.pendingTasks.length > 0 || this.activeTasks.size > 0) && !signal?.aborted) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  /**
   * 队列处理循环
   */
  private async processQueueLoop(): Promise<void> {
    while (this.isRunning) {
      // 等待信号或超时
      await this.waitForSignal(1000);

      if (!this.isRunning) break;

      // 处理待处理任务
      await this.processPendingTasks();
    }
  }

  /**
   * 处理待处理任务
   */
  private async processPendingTasks(): Promise<void> {
    while (
      this.isRunning &&
      this.pendingTasks.length > 0 &&
      this.activeTasks.size < this.config.maxConcurrency
    ) {
      // 优先处理立即执行任务
      const taskIndex = this.pendingTasks.findIndex((t) => t.isImmediateTask);
      const index = taskIndex >= 0 ? taskIndex : 0;

      const task = this.pendingTasks[index];
      this.pendingTasks.splice(index, 1);

      if (task.status === 'cancelled' || task.userCancelled) {
        continue;
      }

      // 添加到活动任务
      const key = this.getTaskKey(task.profileId, task.type);
      this.activeTasks.set(key, task);

      // 执行任务
      this.executeTask(task);
    }
  }

  /**
   * 执行任务
   */
  private async executeTask(task: TransferTask): Promise<void> {
    // 用户取消的任务不执行
    if (task.userCancelled || task.status === 'cancelled') {
      console.log(`[HistoryTransferQueue] Task was cancelled, skipping: ${task.profileId}`);
      return;
    }

    task.status = 'running';
    task.startedTime = Date.now();
    this.notifyStatusChanged(task);

    try {
      if (task.type === 'download') {
        await this.executeDownloadTask(task);
      } else {
        await this.executeUploadTask(task);
      }

      task.status = 'completed';
      task.completedTime = Date.now();
      task.progress = 100;
      this.consecutiveFailures = 0;
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        task.status = 'cancelled';
      } else if (task.userCancelled) {
        task.status = 'cancelled';
        console.log(`[HistoryTransferQueue] Task was cancelled by user: ${task.profileId}`);
      } else {
        task.status = 'failed';
        task.errorMessage = error instanceof Error ? error.message : 'Unknown error';
        task.failureCount++;
        this.consecutiveFailures++;

        console.error(
          `[HistoryTransferQueue] Task failed: ${task.profileId} (${task.type})`,
          error
        );

        // 检查是否需要重试
        if (
          task.failureCount < this.config.maxConsecutiveFailures &&
          this.consecutiveFailures < this.config.maxConsecutiveFailures
        ) {
          task.status = 'waitForRetry';
          setTimeout(() => {
            if (task.status === 'waitForRetry') {
              task.status = 'pending';
              task.abortController = new AbortController();
              this.pendingTasks.push(task);
              this.signalQueue();
            }
          }, this.config.retryDelayMs);
        }
      }
    } finally {
      this.notifyStatusChanged(task);
      const key = this.getTaskKey(task.profileId, task.type);
      this.activeTasks.delete(key);
    }
  }

  /**
   * 执行下载任务
   */
  private async executeDownloadTask(task: TransferTask): Promise<void> {
    if (!this.historyAPI) {
      throw new Error('History API not initialized');
    }

    console.log(`[HistoryTransferQueue] ========== Execute Download Task ==========`);
    console.log(`[HistoryTransferQueue] ProfileId: ${task.profileId}`);
    console.log(`[HistoryTransferQueue] Task created: ${new Date(task.createdTime).toISOString()}`);
    console.log(`[HistoryTransferQueue] Is immediate: ${task.isImmediateTask}`);

    const { parseProfileId } = await import('./HistoryAPI');
    const parsed = parseProfileId(task.profileId);
    if (!parsed) {
      console.error(`[HistoryTransferQueue] Invalid profileId format: ${task.profileId}`);
      throw new Error(`Invalid profileId format: ${task.profileId}`);
    }

    console.log(
      `[HistoryTransferQueue] Parsed profileId - type: ${parsed.type}, hash: ${parsed.hash}`
    );

    const item = await this.historyStorage.getItem(parsed.hash);
    if (!item) {
      console.error(`[HistoryTransferQueue] Item not found with hash: ${parsed.hash}`);
      throw new Error(`Item not found: ${parsed.hash}`);
    }

    console.log(`[HistoryTransferQueue] Item type: ${item.type}`);
    console.log(`[HistoryTransferQueue] Item text (fileName): ${item.text}`);
    console.log(`[HistoryTransferQueue] Item hasRemoteData: ${item.hasRemoteData}`);
    console.log(`[HistoryTransferQueue] Item isLocalFileReady: ${item.isLocalFileReady}`);

    const historyDir = getHistoryFileDir(item.type, parsed.hash);
    if (!historyDir.exists) {
      historyDir.create();
    }

    const fileName = item.type !== 'Text' && item.text ? item.text : 'data';
    const destinationFile = new File(historyDir, fileName);
    const destinationUri = destinationFile.uri;

    console.log(`[HistoryTransferQueue] Destination directory: ${historyDir.uri}`);
    console.log(`[HistoryTransferQueue] Destination file: ${destinationUri}`);

    await this.historyAPI.downloadData(
      task.profileId,
      destinationUri,
      task.abortController.signal,
      (info) => {
        task.bytesTransferred = info.bytesTransferred;
        task.totalBytes = info.totalBytes;
        if (info.progress >= 0) {
          task.progress = Math.round(info.progress * 100);
        } else {
          task.progress = -1;
        }
        this.notifyStatusChanged(task);
      }
    );

    await this.historyStorage.updateItem(parsed.hash, {
      fileUri: destinationUri,
      isLocalFileReady: true,
      // dataName: item.dataName,
      // text: item.text,
    });

    console.log(`[HistoryTransferQueue] Download task completed: ${task.profileId}`);
  }

  /**
   * 执行上传任务
   * 参照桌面客户端实现：
   * 1. 先检查服务器是否已存在该记录
   * 2. 如果存在，直接标记为已同步
   * 3. 如果不存在，执行上传
   */
  private async executeUploadTask(task: TransferTask): Promise<void> {
    if (!this.historyAPI) {
      throw new Error('History API not initialized');
    }

    const { parseProfileId } = await import('./HistoryAPI');
    const parsed = parseProfileId(task.profileId);
    if (!parsed) {
      throw new Error(`Invalid profileId format: ${task.profileId}`);
    }

    const item = await this.historyStorage.getItem(parsed.hash);
    if (!item) {
      throw new Error(`Item not found: ${parsed.hash}`);
    }

    // 先检查服务器上是否已存在该记录
    try {
      const existingRecord = await this.historyAPI.getRecord(
        task.profileId,
        task.abortController.signal
      );
      // 如果服务器记录已删除，视为不存在，继续上传
      if (existingRecord.isDeleted) {
        console.log(
          `[HistoryTransferQueue] Record is deleted on server, will upload: ${task.profileId}`
        );
      } else {
        // 服务器已存在且未删除，直接标记为成功
        await this.historyStorage.updateItem(parsed.hash, {
          version: existingRecord.version,
          lastModified: existingRecord.lastModified
            ? new Date(existingRecord.lastModified).getTime()
            : Date.now(),
          syncStatus: HistorySyncStatus.Synced,
        });
        console.log(
          `[HistoryTransferQueue] Record already exists on server: ${task.profileId}, marked as synced`
        );
        return;
      }
    } catch (error) {
      // 如果是 404 错误，继续执行上传
      if (error instanceof RecordNotFoundError) {
        console.log(
          `[HistoryTransferQueue] Record not found on server, will upload: ${task.profileId}`
        );
      } else {
        throw error;
      }
    }

    // 服务器不存在，执行上传
    if (!item.fileUri) {
      throw new Error(`No file to upload: ${task.profileId}`);
    }

    const { clipboardItemToDto } = await import('./HistoryAPI');
    const dto = clipboardItemToDto(item);

    await this.historyAPI.uploadRecord(dto, item.fileUri, task.abortController.signal, (info) => {
      task.bytesTransferred = info.bytesTransferred;
      task.totalBytes = info.totalBytes;
      if (info.progress >= 0) {
        task.progress = Math.round(info.progress * 100);
      } else {
        task.progress = -1;
      }
      this.notifyStatusChanged(task);
    });

    await this.historyStorage.updateSyncStatus(parsed.hash, HistorySyncStatus.Synced);
  }

  /**
   * 查找任务
   */
  private findTask(profileId: string, type: TransferType): TransferTask | undefined {
    const key = this.getTaskKey(profileId, type);
    const activeTask = this.activeTasks.get(key);
    if (activeTask) return activeTask;

    return this.pendingTasks.find((t) => t.profileId === profileId && t.type === type);
  }

  /**
   * 获取任务键
   */
  private getTaskKey(profileId: string, type: TransferType): string {
    return `${type}:${profileId}`;
  }

  /**
   * 通知状态变化
   */
  private notifyStatusChanged(task: TransferTask): void {
    for (const callback of this.taskStatusCallbacks) {
      try {
        callback(task);
      } catch (error) {
        console.error('[HistoryTransferQueue] Callback error:', error);
      }
    }
  }

  /**
   * 等待队列信号
   */
  private async waitForSignal(timeoutMs: number): Promise<void> {
    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        this.queueSignal = null;
        resolve();
      }, timeoutMs);

      this.queueSignal = {
        resolve: () => {
          clearTimeout(timeout);
          this.queueSignal = null;
          resolve();
        },
      };
    });
  }

  /**
   * 发送队列信号
   */
  private signalQueue(): void {
    if (this.queueSignal) {
      this.queueSignal.resolve();
    }
  }
}

// 单例实例
let historyTransferQueueInstance: HistoryTransferQueue | null = null;

export function getHistoryTransferQueue(): HistoryTransferQueue {
  if (!historyTransferQueueInstance) {
    historyTransferQueueInstance = new HistoryTransferQueue();
  }
  return historyTransferQueueInstance;
}

export function resetHistoryTransferQueue(): void {
  if (historyTransferQueueInstance) {
    historyTransferQueueInstance.stop();
    historyTransferQueueInstance = null;
  }
}
