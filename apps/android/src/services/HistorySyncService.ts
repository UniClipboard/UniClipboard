/**
 * History Sync Service
 * 历史记录同步服务 - 核心同步逻辑
 */

import {
  IHistoryAPI,
  HistoryRecordDto,
  HistoryRecordUpdateDto,
  dtoToClipboardItem,
  SyncConflictError,
  RecordNotFoundError,
  ProfileTypeFilter,
} from './HistoryAPI';
import { HistoryStorage } from './HistoryStorage';
import { ClipboardItem, HistorySyncStatus } from '@/types/clipboard';
import { ServerConfig } from '@/types/api';
import { getSignalRClient, type HistoryChangedEvent } from 'signalr-client';

const MAX_TIME_DIFFERENCE_MS = 5 * 60 * 1000; // 5 分钟

export interface SyncProgress {
  phase: 'fetching' | 'merging' | 'pushing' | 'completed' | 'error';
  total?: number;
  current?: number;
  message?: string;
  error?: string;
}

export interface SyncProgressCallback {
  (progress: SyncProgress): void;
}

export interface HistorySyncConfig {
  serverConfig: ServerConfig;
  historyAPI: IHistoryAPI;
}

export class HistorySyncService {
  private historyStorage: HistoryStorage;
  private historyAPI: IHistoryAPI | null = null;
  private serverConfig: ServerConfig | null = null;
  private lastSyncTime: number | null = null;
  private isSyncing = false;
  private syncAbortController: AbortController | null = null;
  private reorganizeAbortController: AbortController | null = null;
  private progressCallbacks: Set<SyncProgressCallback> = new Set();
  private storageChangeCallback:
    | ((items: ClipboardItem[], action: 'add' | 'update' | 'delete') => void)
    | null = null;

  constructor() {
    this.historyStorage = HistoryStorage.getInstance();
  }

  /**
   * 检查历史记录同步是否启用
   */
  private async isHistorySyncEnabled(): Promise<boolean> {
    const { configStorage } = await import('./ConfigStorage');
    const config = await configStorage.getConfig();
    return config?.enableHistorySync ?? false;
  }

  /**
   * 初始化同步服务
   */
  async initialize(config: HistorySyncConfig): Promise<void> {
    this.historyAPI = config.historyAPI;
    this.serverConfig = config.serverConfig;

    // 注册 SignalR 历史变化事件监听
    const client = getSignalRClient();
    client.onRemoteHistoryChanged(this.handleNativeHistoryChanged);

    // 注册本地存储变更回调，自动同步 NeedSync 记录
    this.storageChangeCallback = this.handleLocalHistoryChanged;
    this.historyStorage.addChangeCallback(this.storageChangeCallback);

    // 加载上次同步时间
    await this.loadLastSyncTime();

    // 初始化传输队列
    const { getHistoryTransferQueue } = await import('./HistoryTransferQueue');
    const transferQueue = getHistoryTransferQueue();
    transferQueue.setHistoryAPI(config.historyAPI);
    await transferQueue.start();
  }

  /**
   * 检查服务是否已初始化
   */
  isInitialized(): boolean {
    return this.historyAPI !== null;
  }

  /**
   * 销毁同步服务
   */
  async destroy(): Promise<void> {
    // 移除 SignalR 历史变化回调
    const client = getSignalRClient();
    client.offRemoteHistoryChanged(this.handleNativeHistoryChanged);

    // 移除本地存储变更回调
    if (this.storageChangeCallback) {
      this.historyStorage.removeChangeCallback(this.storageChangeCallback);
      this.storageChangeCallback = null;
    }

    this.historyAPI = null;
    this.serverConfig = null;
    this.lastSyncTime = null;
    this.cancelSync();

    // 停止传输队列
    const { getHistoryTransferQueue } = await import('./HistoryTransferQueue');
    const transferQueue = getHistoryTransferQueue();
    await transferQueue.stop();
  }

  /**
   * 切换服务器
   */
  async switchServer(config: HistorySyncConfig): Promise<void> {
    // 取消当前同步
    this.cancelSync();

    // 重置同步时间，触发全量同步
    this.lastSyncTime = null;
    await this.saveLastSyncTime();

    // 更新配置
    this.historyAPI = config.historyAPI;
    this.serverConfig = config.serverConfig;

    // 更新传输队列 API
    const { getHistoryTransferQueue } = await import('./HistoryTransferQueue');
    const transferQueue = getHistoryTransferQueue();
    transferQueue.setHistoryAPI(config.historyAPI);

    // 触发同步
    await this.syncAll();
  }

  /**
   * 全量同步
   */
  async syncAll(progressCallback?: SyncProgressCallback): Promise<void> {
    return this.executeSync(true, progressCallback);
  }

  async syncIncremental(progressCallback?: SyncProgressCallback): Promise<void> {
    return this.executeSync(false, progressCallback);
  }

  private async executeSync(
    isFullSync: boolean,
    progressCallback?: SyncProgressCallback
  ): Promise<void> {
    const lastSyncTime = isFullSync ? undefined : (this.lastSyncTime ?? undefined);

    if (this.isSyncing) {
      console.log('[HistorySyncService] Sync already in progress');
      return;
    }

    if (!this.historyAPI) {
      throw new Error('History API not initialized');
    }

    if (!(await this.isHistorySyncEnabled())) {
      console.log('[HistorySyncService] Sync is disabled');
      return;
    }

    this.isSyncing = true;
    this.syncAbortController = new AbortController();

    if (progressCallback) {
      this.progressCallbacks.add(progressCallback);
    }

    try {
      await this.notifyProgress({ phase: 'fetching', message: '校验服务器时间...' });
      await this.validateServerTime(this.syncAbortController.signal);

      await this.notifyProgress({ phase: 'fetching', message: '获取远程记录...' });
      const remoteRecords = await this.fetchRemoteRecords(
        this.syncAbortController.signal,
        lastSyncTime
      );

      if (remoteRecords.length > 0) {
        await this.notifyProgress({
          phase: 'merging',
          message: '合并记录...',
          total: remoteRecords.length,
        });
        await this.mergeRemoteRecords(remoteRecords, this.syncAbortController.signal);
      }

      if (isFullSync) {
        await this.notifyProgress({ phase: 'merging', message: '检测孤儿数据...' });
        await this.detectOrphanData(remoteRecords, this.syncAbortController.signal);
      }

      await this.notifyProgress({ phase: 'pushing', message: '推送本地变更...' });
      await this.pushLocalChanges(this.syncAbortController.signal);

      await this.notifyProgress({ phase: 'pushing', message: '上传本地记录...' });
      await this.pushLocalOnlyRecords(this.syncAbortController.signal);

      await this.notifyProgress({ phase: 'pushing', message: '清理过期记录...' });
      const cleanedCount = await this.historyStorage.cleanupExpiredSoftDeletes();
      if (cleanedCount > 0) {
        console.log(`[HistorySyncService] Cleaned ${cleanedCount} expired soft-deleted records`);
      }

      this.lastSyncTime = Date.now();
      await this.saveLastSyncTime();

      await this.notifyProgress({ phase: 'completed', message: '同步完成' });
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        console.log('[HistorySyncService] Sync cancelled');
        await this.notifyProgress({ phase: 'error', message: '同步已取消' });
      } else {
        console.error('[HistorySyncService] Sync failed:', error);
        await this.notifyProgress({
          phase: 'error',
          message: '同步失败',
          error: error instanceof Error ? error.message : 'Unknown error',
        });
        throw error;
      }
    } finally {
      this.isSyncing = false;
      this.syncAbortController = null;
      if (progressCallback) {
        this.progressCallbacks.delete(progressCallback);
      }
    }
  }

  /**
   * 取消同步
   */
  cancelSync(): void {
    if (this.syncAbortController) {
      this.syncAbortController.abort();
    }
  }

  /**
   * 取消整理
   */
  cancelReorganize(): void {
    if (this.reorganizeAbortController) {
      this.reorganizeAbortController.abort();
    }
  }

  /**
   * 取消所有操作（同步和整理）
   */
  cancelAll(): void {
    this.cancelSync();
    this.cancelReorganize();
  }

  /**
   * 获取整理操作的 AbortController
   */
  getReorganizeAbortController(): AbortController | null {
    return this.reorganizeAbortController;
  }

  /**
   * 设置整理操作的 AbortController
   */
  setReorganizeAbortController(controller: AbortController | null): void {
    this.reorganizeAbortController = controller;
  }

  /**
   * 添加进度回调
   */
  addProgressCallback(callback: SyncProgressCallback): void {
    this.progressCallbacks.add(callback);
  }

  /**
   * 移除进度回调
   */
  removeProgressCallback(callback: SyncProgressCallback): void {
    this.progressCallbacks.delete(callback);
  }

  /**
   * 获取上次同步时间
   */
  getLastSyncTime(): number | null {
    return this.lastSyncTime;
  }

  async resetSyncCursor(): Promise<void> {
    this.lastSyncTime = null;
    await this.saveLastSyncTime();
  }

  /**
   * 是否正在同步
   */
  isSyncInProgress(): boolean {
    return this.isSyncing;
  }

  /**
   * 校验服务器时间
   */
  private async validateServerTime(_signal: AbortSignal): Promise<void> {
    if (!this.historyAPI) return;

    try {
      const serverTime = await this.historyAPI.getServerTime?.();
      if (serverTime) {
        const localTime = new Date();
        const diff = Math.abs(localTime.getTime() - serverTime.getTime());

        if (diff > MAX_TIME_DIFFERENCE_MS) {
          console.warn(
            `[HistorySyncService] Time difference detected: ${diff}ms. ` +
              `Local: ${localTime.toISOString()}, Server: ${serverTime.toISOString()}`
          );
          // 移动端只警告，不阻止同步
        }
      }
    } catch (error) {
      console.warn('[HistorySyncService] Failed to validate server time:', error);
      // 不阻止同步
    }
  }

  private async fetchRemoteRecords(
    signal: AbortSignal,
    modifiedAfter?: number
  ): Promise<HistoryRecordDto[]> {
    if (!this.historyAPI) return [];

    const isIncremental = modifiedAfter !== undefined;
    const progressMessage = isIncremental ? '获取增量记录' : '获取远程记录';
    const allRecords: HistoryRecordDto[] = [];
    let page = 1;
    let hasMore = true;

    while (hasMore) {
      if (signal.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }

      const queryOptions: Parameters<typeof this.historyAPI.queryRecords>[0] = {
        page,
        types: ProfileTypeFilter.All,
      };

      if (isIncremental) {
        queryOptions.modifiedAfter = new Date(modifiedAfter).toISOString();
      }

      const records = await this.historyAPI.queryRecords(queryOptions, signal);

      if (records.length === 0) {
        hasMore = false;
      } else {
        allRecords.push(...records);
        page++;

        await this.notifyProgress({
          phase: 'fetching',
          message: `${progressMessage}... (${allRecords.length} 条)`,
          current: allRecords.length,
        });
      }
    }

    console.log(
      `[HistorySyncService] ${isIncremental ? 'Incremental' : 'Total'} records fetched: ${allRecords.length}`
    );
    return allRecords;
  }

  /**
   * 合并远程记录到本地
   */
  private async mergeRemoteRecords(
    remoteRecords: HistoryRecordDto[],
    signal: AbortSignal
  ): Promise<void> {
    // 先收集所有需要添加和更新的记录
    const itemsToAdd: ClipboardItem[] = [];
    const itemsToUpdate: { profileHash: string; updates: Partial<ClipboardItem> }[] = [];

    // 统计计数
    let remoteAddedCount = 0;
    let remoteUpdatedCount = 0;
    let localUpdatedCount = 0;

    const localItems = await this.historyStorage.getAllItemsIncludingDeleted();
    const localMap = new Map<string, ClipboardItem>();

    for (const item of localItems) {
      localMap.set(item.profileHash.toLowerCase(), item);
    }

    let processed = 0;
    for (const remoteRecord of remoteRecords) {
      if (signal.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }

      const localItem = localMap.get(remoteRecord.hash.toLowerCase());

      if (!localItem) {
        // 本地不存在，收集待添加（跳过已删除的记录）
        if (!remoteRecord.isDeleted) {
          itemsToAdd.push(dtoToClipboardItem(remoteRecord));
          remoteAddedCount++;
        }
      } else {
        // 本地存在，检查是否需要更新
        const remoteVersion = remoteRecord.version || 0;
        const localVersion = localItem.version || 0;
        const remoteModified = remoteRecord.lastModified
          ? new Date(remoteRecord.lastModified).getTime()
          : 0;
        const localModified = localItem.lastModified || 0;

        const TIME_THRESHOLD_MS = 5 * 60 * 1000; // 5 分钟
        const timeDiff = Math.abs(remoteModified - localModified);

        let shouldUpdateFromRemote = false;
        let isLocalNewer = false;

        if (timeDiff > TIME_THRESHOLD_MS) {
          shouldUpdateFromRemote = remoteModified > localModified;
          isLocalNewer = localModified > remoteModified;
        } else {
          shouldUpdateFromRemote = remoteVersion > localVersion;
          isLocalNewer = localVersion > remoteVersion;
        }

        if (shouldUpdateFromRemote) {
          // 远程更新，更新本地数据
          itemsToUpdate.push({
            profileHash: localItem.profileHash,
            updates: {
              text: remoteRecord.text || localItem.text,
              starred: remoteRecord.starred,
              pinned: remoteRecord.pinned,
              version: remoteVersion,
              lastModified: remoteModified,
              syncStatus: HistorySyncStatus.Synced,
              hasRemoteData: remoteRecord.hasData,
              isLocalFileReady: localItem.isLocalFileReady,
              fileUri: localItem.fileUri,
              isDeleted: remoteRecord.isDeleted || false,
            },
          });
          remoteUpdatedCount++;
        } else if (isLocalNewer) {
          // 本地更新，标记为需要同步
          // 但 hasData === true 的记录无法上传，保持 LocalOnly
          itemsToUpdate.push({
            profileHash: localItem.profileHash,
            updates: {
              syncStatus: localItem.hasData
                ? HistorySyncStatus.LocalOnly
                : HistorySyncStatus.NeedSync,
            },
          });
          if (!localItem.hasData) {
            localUpdatedCount++;
          }
        } else {
          // 版本相同，标记为已同步
          itemsToUpdate.push({
            profileHash: localItem.profileHash,
            updates: {
              syncStatus: HistorySyncStatus.Synced,
            },
          });
        }
      }

      processed++;
      if (processed % 50 === 0) {
        await this.notifyProgress({
          phase: 'merging',
          current: processed,
          total: remoteRecords.length,
        });
      }
    }

    // 批量执行持久化操作
    if (itemsToAdd.length > 0) {
      await this.historyStorage.addItems(itemsToAdd);
    }
    if (itemsToUpdate.length > 0) {
      await this.historyStorage.updateItems(itemsToUpdate);
    }

    console.log(
      `[HistorySyncService] Merge completed - Remote added: ${remoteAddedCount}, Remote updated: ${remoteUpdatedCount}, Local updated (need sync): ${localUpdatedCount}`
    );
  }

  /**
   * 检测孤儿数据
   * 本地标记为 Synced 但服务器不存在的记录，或本地无数据(isLocalFileReady=false)但服务器不存在的记录
   */
  private async detectOrphanData(
    remoteRecords: HistoryRecordDto[],
    signal: AbortSignal
  ): Promise<void> {
    const localItems = await this.historyStorage.getAllItemsIncludingDeleted();
    const remoteIds = new Set(remoteRecords.map((r) => r.hash.toLowerCase()));

    for (const localItem of localItems) {
      if (signal.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }

      // 跳过软删除的记录（由过期清理机制处理）
      if (localItem.isDeleted) {
        continue;
      }

      // 只处理已同步的记录或本地无数据的记录
      const isSynced = localItem.syncStatus === HistorySyncStatus.Synced;
      const isServerOnly = localItem.isLocalFileReady === false;

      if (!isSynced && !isServerOnly) {
        continue;
      }

      // 如果远程不存在
      if (!remoteIds.has(localItem.profileHash.toLowerCase())) {
        if (isServerOnly) {
          // 本地无数据，直接物理删除
          await this.historyStorage.physicalDeleteItem(localItem.profileHash);
          console.log(
            `[HistorySyncService] Orphan record deleted (no local data): ${localItem.profileHash}`
          );
        } else if (localItem.isLocalFileReady) {
          // 本地有数据，标记为 LocalOnly
          await this.historyStorage.updateSyncStatus(
            localItem.profileHash,
            HistorySyncStatus.LocalOnly
          );
        } else {
          // 本地无数据，直接物理删除
          await this.historyStorage.physicalDeleteItem(localItem.profileHash);
          console.log(`[HistorySyncService] Orphan record deleted: ${localItem.profileHash}`);
        }
      }
    }
  }

  /**
   * 推送本地变更到服务器
   * 注意：移动端不上传有数据的记录，只同步元数据
   */
  private async pushLocalChanges(signal: AbortSignal): Promise<void> {
    if (!this.historyAPI) return;

    const needSyncItems = await this.historyStorage.getNeedSyncItems();
    let successCount = 0;
    let conflictCount = 0;
    let notFoundCount = 0;

    for (const item of needSyncItems) {
      if (signal.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }

      try {
        const result = await this.pushRecordUpdate(item, signal);
        if (result === 'synced') {
          successCount++;
        } else if (result === 'conflict') {
          conflictCount++;
        } else if (result === 'notFound') {
          notFoundCount++;
        }
      } catch (error) {
        if (error instanceof Error && error.name === 'AbortError') {
          throw error;
        }
        console.error(`[HistorySyncService] Failed to push record ${item.profileHash}:`, error);
        throw error;
      }
    }

    console.log(
      `[HistorySyncService] Push local changes completed - Success: ${successCount}, Conflict: ${conflictCount}, Not found: ${notFoundCount}`
    );
  }

  /**
   * 推送 LocalOnly 记录到服务器
   * - hasData === true: 不上传（有数据文件，移动端不支持上传大文件）
   * - hasData === false: 上传元数据（纯文本，不需要数据文件）
   */
  private async pushLocalOnlyRecords(signal: AbortSignal): Promise<void> {
    if (!this.historyAPI) return;

    const localOnlyItems = await this.historyStorage.getLocalOnlyItems();

    for (const item of localOnlyItems) {
      if (signal.aborted) {
        throw new DOMException('Aborted', 'AbortError');
      }

      // 跳过已删除的记录
      if (item.isDeleted) {
        continue;
      }

      // hasData === true: 有数据文件，不上传
      if (item.hasData) {
        console.log(
          `[HistorySyncService] Skipping LocalOnly record with data: ${item.profileHash}`
        );
        continue;
      }

      // hasData === false: 上传元数据
      try {
        const { clipboardItemToDto } = await import('./HistoryAPI');
        const dto = clipboardItemToDto(item);

        console.log(`[HistorySyncService] Uploading LocalOnly record: ${item.profileHash}`);
        const createdRecord = await this.historyAPI.uploadRecord(dto, undefined, signal);

        // 更新本地状态为已同步
        await this.historyStorage.updateItem(item.profileHash, {
          version: createdRecord.version,
          lastModified: createdRecord.lastModified
            ? new Date(createdRecord.lastModified).getTime()
            : Date.now(),
          syncStatus: HistorySyncStatus.Synced,
          hasRemoteData: false,
        });

        console.log(`[HistorySyncService] LocalOnly record uploaded: ${item.profileHash}`);
      } catch (error) {
        if (error instanceof SyncConflictError) {
          // 预期错误：记录已存在于服务器，使用服务器版本
          console.log(
            `[HistorySyncService] LocalOnly record already exists on server: ${item.profileHash}`
          );
          await this.handleSyncConflict(item, error.serverRecord);
        } else if (error instanceof Error && error.name === 'AbortError') {
          // 取消同步，传递到外层
          throw error;
        } else {
          // 非预期错误，传递到外层
          console.error(
            `[HistorySyncService] Failed to upload LocalOnly record ${item.profileHash}:`,
            error
          );
          throw error;
        }
      }
    }
  }

  /**
   * 处理同步冲突
   */
  private async handleSyncConflict(
    local: ClipboardItem,
    serverRecord: HistoryRecordDto
  ): Promise<void> {
    const remoteVersion = serverRecord.version || 0;

    // 更新本地元数据，保留本地数据状态
    await this.historyStorage.updateItem(local.profileHash, {
      text: serverRecord.text || local.text,
      starred: serverRecord.starred,
      pinned: serverRecord.pinned,
      version: remoteVersion,
      lastModified: serverRecord.lastModified
        ? new Date(serverRecord.lastModified).getTime()
        : Date.now(),
      syncStatus: HistorySyncStatus.Synced,
      hasRemoteData: serverRecord.hasData,
      isLocalFileReady: local.isLocalFileReady,
      fileUri: local.fileUri,
      isDeleted: serverRecord.isDeleted,
    });
    console.log(
      `[HistorySyncService] Conflict resolved for ${local.profileHash}, using server version`
    );
  }

  /**
   * 处理本地历史变更
   * - NeedSync 记录：同步元数据更新到服务器
   * - 新增记录：上传到服务器（如果启用了历史记录同步）
   */
  private handleLocalHistoryChanged = async (
    items: ClipboardItem[],
    action: 'add' | 'update' | 'delete'
  ): Promise<void> => {
    if (!this.historyAPI || !(await this.isHistorySyncEnabled())) return;

    for (const item of items) {
      try {
        if (item.syncStatus === HistorySyncStatus.NeedSync) {
          await this.syncOneRecord(item);
        } else if (action === 'add' && item.syncStatus === HistorySyncStatus.LocalOnly) {
          await this.uploadNewRecord(item);
        }
      } catch (error) {
        console.error(`[HistorySyncService] Failed to sync record ${item.profileHash}:`, error);
      }
    }
  };

  /**
   * 上传新记录到服务器
   * - hasData === true: 不上传（有数据文件，移动端不支持上传大文件）
   * - hasData === false: 上传元数据（纯文本，不需要数据文件）
   */
  private async uploadNewRecord(item: ClipboardItem): Promise<void> {
    if (!this.historyAPI) return;

    if (item.hasData) {
      console.log(
        `[HistorySyncService] Skipping new record with data (mobile does not support file upload): ${item.profileHash}`
      );
      return;
    }

    try {
      const { clipboardItemToDto } = await import('./HistoryAPI');
      const dto = clipboardItemToDto(item);

      const createdRecord = await this.historyAPI.uploadRecord(dto);

      await this.historyStorage.updateItem(item.profileHash, {
        version: createdRecord.version,
        lastModified: createdRecord.lastModified
          ? new Date(createdRecord.lastModified).getTime()
          : Date.now(),
        syncStatus: HistorySyncStatus.Synced,
        hasRemoteData: false,
      });

      console.log(`[HistorySyncService] New record uploaded: ${item.profileHash}`);
    } catch (error) {
      if (error instanceof SyncConflictError) {
        await this.handleSyncConflict(item, error.serverRecord);
      } else {
        throw error;
      }
    }
  }

  /**
   * 推送记录更新到服务器（统一处理返回结果）
   * 1. 返回 OK，将本地记录标记为已同步
   * 2. 返回 404，将本地记录标记为 LocalOnly
   * 3. 返回冲突，以服务器记录为准更新本地记录，并标记为已同步
   * 4. 其他异常，原样抛出
   */
  private async pushRecordUpdate(
    item: ClipboardItem,
    signal?: AbortSignal
  ): Promise<'synced' | 'notFound' | 'conflict'> {
    if (!this.historyAPI) {
      throw new Error('History API not initialized');
    }

    const type = item.type as 'Text' | 'Image' | 'File';
    const update: HistoryRecordUpdateDto = {
      starred: item.starred,
      pinned: item.pinned,
      isDelete: item.isDeleted,
      version: item.version,
      lastModified: item.lastModified ? new Date(item.lastModified).toISOString() : undefined,
    };

    try {
      const updatedRecord = await this.historyAPI.updateRecord(
        type,
        item.profileHash,
        update,
        signal
      );

      await this.historyStorage.updateItem(item.profileHash, {
        version: updatedRecord.version,
        lastModified: updatedRecord.lastModified
          ? new Date(updatedRecord.lastModified).getTime()
          : Date.now(),
        syncStatus: HistorySyncStatus.Synced,
      });
      return 'synced';
    } catch (error) {
      if (error instanceof SyncConflictError) {
        await this.handleSyncConflict(item, error.serverRecord);
        return 'conflict';
      } else if (error instanceof RecordNotFoundError) {
        await this.historyStorage.updateItem(item.profileHash, {
          syncStatus: HistorySyncStatus.LocalOnly,
        });
        return 'notFound';
      }
      throw error;
    }
  }

  /**
   * 同步单条记录
   */
  private async syncOneRecord(item: ClipboardItem): Promise<void> {
    if (!this.historyAPI) return;

    try {
      await this.pushRecordUpdate(item);
    } catch (error) {
      console.error(`[HistorySyncService] Failed to sync record ${item.profileHash}:`, error);
    }
  }

  /**
   * 处理远程历史变化
   */
  /**
   * 处理 native SignalR 历史变化事件，转换为 HistoryRecordDto 并处理
   */
  private handleNativeHistoryChanged = async (event: HistoryChangedEvent): Promise<void> => {
    const record: HistoryRecordDto = {
      hash: event.hash,
      text: event.text,
      type: event.type as 'Text' | 'Image' | 'File',
      hasData: event.hasData,
      size: event.size,
      starred: event.starred,
      pinned: event.pinned,
      version: event.version,
      isDeleted: event.isDeleted,
      createTime: event.createTime,
      lastModified: event.lastModified,
      lastAccessed: event.lastAccessed,
    };
    await this.handleRemoteHistoryChanged(record);
  };

  private handleRemoteHistoryChanged = async (record: HistoryRecordDto): Promise<void> => {
    console.log('[HistorySyncService] Remote history changed:', record.hash);

    // 如果服务器标记为已删除
    if (record.isDeleted) {
      const localItem = await this.historyStorage.getItem(record.hash);
      if (localItem) {
        // 标记本地为软删除
        await this.historyStorage.updateItem(localItem.profileHash, {
          isDeleted: true,
          version: record.version || 0,
          lastModified: record.lastModified ? new Date(record.lastModified).getTime() : Date.now(),
          syncStatus: HistorySyncStatus.Synced,
          isLocalFileReady: false,
        });
        console.log(`[HistorySyncService] Remote record deleted: ${record.hash}`);
      }
      return;
    }

    const localItem = await this.historyStorage.getItem(record.hash);

    if (!localItem) {
      // 新记录，添加到本地
      const item = dtoToClipboardItem(record);
      await this.historyStorage.addItem(item);
    } else {
      // 已存在，合并
      const remoteVersion = record.version || 0;
      const localVersion = localItem.version || 0;

      if (remoteVersion > localVersion || !localItem.lastModified) {
        const remoteModified = record.lastModified ? new Date(record.lastModified).getTime() : 0;
        const localModified = localItem.lastModified || 0;

        if (remoteModified > localModified) {
          await this.historyStorage.updateItem(localItem.profileHash, {
            text: record.text || localItem.text,
            starred: record.starred,
            pinned: record.pinned,
            version: remoteVersion,
            lastModified: remoteModified,
            syncStatus: HistorySyncStatus.Synced,
            hasRemoteData: record.hasData,
            isLocalFileReady: localItem.isLocalFileReady,
            fileUri: localItem.fileUri,
            isDeleted: false,
          });
        }
      }
    }
  };

  /**
   * 通知进度
   */
  private async notifyProgress(progress: SyncProgress): Promise<void> {
    for (const callback of this.progressCallbacks) {
      try {
        callback(progress);
      } catch (error) {
        console.error('[HistorySyncService] Progress callback error:', error);
      }
    }
  }

  /**
   * 加载上次同步时间
   */
  private async loadLastSyncTime(): Promise<void> {
    try {
      const AsyncStorage = require('@react-native-async-storage/async-storage').default;
      const timeStr = await AsyncStorage.getItem('@syncclipboard:history:last_sync_time');
      if (timeStr) {
        this.lastSyncTime = parseInt(timeStr, 10);
      }
    } catch (error) {
      console.warn('[HistorySyncService] Failed to load last sync time:', error);
    }
  }

  /**
   * 保存上次同步时间
   */
  private async saveLastSyncTime(): Promise<void> {
    try {
      const AsyncStorage = require('@react-native-async-storage/async-storage').default;
      if (this.lastSyncTime) {
        await AsyncStorage.setItem(
          '@syncclipboard:history:last_sync_time',
          this.lastSyncTime.toString()
        );
      } else {
        await AsyncStorage.removeItem('@syncclipboard:history:last_sync_time');
      }
    } catch (error) {
      console.warn('[HistorySyncService] Failed to save last sync time:', error);
    }
  }

  /**
   * 同步记录更新到服务器（通用方法）
   * 1. 返回 OK，将本地记录标记为已同步
   * 2. 返回 404，将本地记录标记为 LocalOnly
   * 3. 返回冲突，以服务器记录为准更新本地记录，并标记为已同步
   * 4. 其他异常，原样抛出
   */
  async syncRecordUpdate(
    type: 'Text' | 'Image' | 'File',
    hash: string,
    update: HistoryRecordUpdateDto
  ): Promise<'synced' | 'notFound' | 'conflict'> {
    if (!this.historyAPI) {
      console.warn('[HistorySyncService] History API not initialized, cannot sync');
      throw new Error('History API not initialized');
    }

    try {
      const updatedRecord = await this.historyAPI.updateRecord(type, hash, update);
      console.log(`[HistorySyncService] Record synced for ${hash}`);

      await this.historyStorage.updateItem(hash, {
        version: updatedRecord.version,
        lastModified: updatedRecord.lastModified
          ? new Date(updatedRecord.lastModified).getTime()
          : Date.now(),
        syncStatus: HistorySyncStatus.Synced,
      });
      return 'synced';
    } catch (error) {
      if (error instanceof SyncConflictError) {
        const localItem = await this.historyStorage.getItem(hash);
        if (localItem) {
          await this.handleSyncConflict(localItem, error.serverRecord);
        }
        return 'conflict';
      } else if (error instanceof RecordNotFoundError) {
        await this.historyStorage.updateItem(hash, {
          syncStatus: HistorySyncStatus.LocalOnly,
        });
        return 'notFound';
      }
      console.error(`[HistorySyncService] Failed to sync record ${hash}:`, error);
      throw error;
    }
  }

  /**
   * 同步收藏状态到服务器
   */
  async syncStarStatus(
    type: 'Text' | 'Image' | 'File',
    hash: string,
    starred: boolean
  ): Promise<'synced' | 'notFound' | 'conflict'> {
    return this.syncRecordUpdate(type, hash, { starred });
  }

  /**
   * 同步置顶状态到服务器
   */
  async syncPinStatus(
    type: 'Text' | 'Image' | 'File',
    hash: string,
    pinned: boolean
  ): Promise<'synced' | 'notFound' | 'conflict'> {
    return this.syncRecordUpdate(type, hash, { pinned });
  }

  /**
   * 同步删除状态到服务器
   */
  async syncDeleteStatus(
    type: 'Text' | 'Image' | 'File',
    hash: string
  ): Promise<'synced' | 'notFound' | 'conflict'> {
    return this.syncRecordUpdate(type, hash, { isDelete: true });
  }

  async cleanupRemoteHistorys(signal?: AbortSignal): Promise<void> {
    console.log('[HistorySyncService] Cleaning up remote history records...');
    const localItems = await this.historyStorage.getAllItemsIncludingDeleted();

    const toDeleteHashes: string[] = [];
    const toMarkAsLocalOnly: string[] = [];

    for (const item of localItems) {
      if (signal?.aborted) {
        console.log('[HistorySyncService] Cleanup aborted');
        throw new DOMException('Aborted', 'AbortError');
      }

      if (item.isDeleted) {
        continue;
      }

      const hasRemoteData = item.hasRemoteData === true;
      const noLocalData = item.isLocalFileReady === false;

      if (hasRemoteData && noLocalData) {
        toDeleteHashes.push(item.profileHash);
      } else if (
        item.syncStatus === HistorySyncStatus.Synced ||
        item.syncStatus === HistorySyncStatus.NeedSync ||
        item.syncStatus === undefined
      ) {
        toMarkAsLocalOnly.push(item.profileHash);
      }
    }

    if (toDeleteHashes.length > 0) {
      console.log(`[HistorySyncService] Deleting ${toDeleteHashes.length} server-only records...`);
      await this.historyStorage.physicalDeleteItems(toDeleteHashes);
    }

    if (toMarkAsLocalOnly.length > 0) {
      console.log(
        `[HistorySyncService] Marking ${toMarkAsLocalOnly.length} records as LocalOnly...`
      );
      await this.historyStorage.updateItems(
        toMarkAsLocalOnly.map((hash) => ({
          profileHash: hash,
          updates: { syncStatus: HistorySyncStatus.LocalOnly },
        }))
      );
    }

    console.log(
      `[HistorySyncService] Remote history cleanup completed: deleted=${toDeleteHashes.length}, marked=${toMarkAsLocalOnly.length}`
    );
  }

  async ensureInitialized(serverConfig: ServerConfig): Promise<boolean> {
    if (this.isInitialized()) {
      const serverChanged =
        this.serverConfig?.url !== serverConfig.url ||
        this.serverConfig?.username !== serverConfig.username;

      if (serverChanged) {
        console.log('[HistorySyncService] Server changed, switching...');
        const { SyncClipboardClient } = await import('./SyncClipboardClient');
        const { AuthService } = await import('./AuthService');
        const authService =
          serverConfig.username && serverConfig.password
            ? new AuthService(serverConfig.username, serverConfig.password)
            : undefined;

        const historyAPI = new SyncClipboardClient({
          baseURL: serverConfig.url,
          authService,
        });

        await this.switchServer({
          serverConfig,
          historyAPI,
        });
      }
      return true;
    }

    const { SyncClipboardClient } = await import('./SyncClipboardClient');
    const { AuthService } = await import('./AuthService');
    const authService =
      serverConfig.username && serverConfig.password
        ? new AuthService(serverConfig.username, serverConfig.password)
        : undefined;

    const historyAPI = new SyncClipboardClient({
      baseURL: serverConfig.url,
      authService,
    });

    await this.initialize({
      serverConfig,
      historyAPI,
    });

    return true;
  }
}

// 单例实例
let historySyncServiceInstance: HistorySyncService | null = null;

export function getHistorySyncService(): HistorySyncService {
  if (!historySyncServiceInstance) {
    historySyncServiceInstance = new HistorySyncService();
  }
  return historySyncServiceInstance;
}

export function resetHistorySyncService(): void {
  if (historySyncServiceInstance) {
    historySyncServiceInstance.destroy();
    historySyncServiceInstance = null;
  }
}
