/**
 * ClipboardSyncService
 * 管理远程剪贴板同步（前台显示 + 后台同步）。
 *
 * 职责：
 * - 远程剪贴板获取（SignalR 或定时轮询）
 * - 处理远程剪贴板变化（哈希检测、自动下载、自动复制、历史记录）
 * - SyncManager 生命周期管理（初始化/销毁）
 * - HistorySyncService 初始化
 * - 自动上传（本地剪贴板变化时触发）
 * - 通过 useClipboardSyncServiceStore 向 UI 提供状态
 *
 * 生命周期仅由 BackgroundServiceManager 控制。
 * HomeScreen 通过 useClipboardSyncServiceStore 读取状态，通过公开方法触发用户操作。
 */

import { Platform, ToastAndroid } from 'react-native';
import {
  ClipboardContent,
  createDefaultClipboardItem,
  HistorySyncStatus,
} from '../types/clipboard';
import { SyncDirection, SyncResult } from '../types/sync';
import type { ProfileChangedEvent } from 'signalr-client';
import type { ServerConfig } from '../types/api';
import type { ISyncClipboardAPI } from './APIClient';

class ClipboardSyncService {
  private static instance: ClipboardSyncService | null = null;

  /** start() 是否已执行（幂等保护，防止 App.tsx 与 BackgroundServiceManager 双重启动） */
  private _isStarted = false;
  private activeServer: ServerConfig | null = null;
  private pollingTag: string | null = null;
  private signalRConnected = false;
  private lastRemoteProfileHash: string | null = null;
  private lastLocalProfileHash: string | null = null;
  private isAutoSyncing = false;
  private clipboardUnsub: (() => void) | null = null;
  private historyUnsub: (() => void) | null = null;
  private transferQueueHandler:
    | ((task: import('./HistoryTransferQueue').TransferTask) => Promise<void>)
    | null = null;
  /** App 是否在前台（影响自动复制策略） */
  private isAppActive = true;
  /** 当前正在进行的远程文件下载 AbortController */
  private _downloadAbortController: AbortController | null = null;
  /** 当前正在进行的剪贴板上传 AbortController */
  private _uploadAbortController: AbortController | null = null;

  /** SignalR 收到远程变化时的统一回调（箭头函数保证 this 稳定，供 off 注销） */
  private readonly _signalRCallback = async (event: ProfileChangedEvent): Promise<void> => {
    try {
      if (!this.activeServer) return;
      const { profileDtoToContent } = await import('../utils/clipboard');
      const profile = {
        type: event.type as 'Text' | 'Image' | 'File' | 'Group',
        hash: event.hash,
        text: event.text,
        hasData: event.hasData,
        dataName: event.dataName,
        size: event.size,
      };
      const content = profileDtoToContent(profile);
      const currentHash = content.profileHash || content.text || '';
      const { createAPIClient } = require('./index');
      const apiClient = createAPIClient(this.activeServer);
      await this._processRemoteClipboardContent(
        content,
        currentHash,
        profile.hasData,
        apiClient,
        'SignalR: '
      );
    } catch (e) {
      console.error('[ClipboardSyncService] SignalR callback error:', e);
    }
  };

  private constructor() {}

  static getInstance(): ClipboardSyncService {
    if (!ClipboardSyncService.instance) {
      ClipboardSyncService.instance = new ClipboardSyncService();
    }
    return ClipboardSyncService.instance;
  }

  // ─── 生命周期（由 BackgroundServiceManager 调用）──────────────────────────

  /**
   * 启动同步服务（幂等：若已启动则直接返回）。
   * 由 App.tsx 在 isLoaded 后统一调用（所有平台）。
   */
  async start(): Promise<void> {
    if (this._isStarted) return;
    this._isStarted = true;

    const { useSettingsStore } = require('../stores/settingsStore');
    if (!useSettingsStore.getState().isLoaded) {
      await useSettingsStore.getState().loadConfig();
    }
    const activeServer = useSettingsStore.getState().getActiveServer();

    // 初始化 SyncManager
    await this._initializeSyncManager();

    // 初始化历史同步服务
    if (activeServer) {
      await this._initializeHistorySyncService(activeServer);
    }

    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');

    if (!activeServer) {
      useClipboardSyncServiceStore.getState().setRemoteContent(null);
      this._subscribeToClipboardChanges();
      return;
    }

    this.activeServer = activeServer;
    this.lastRemoteProfileHash = null; // 重启时重置，确保首次获取能正确处理

    // 建立远程连接（SignalR 或轮询）
    await this._startConnection(activeServer);

    // 订阅本地剪贴板变化（用于自动上传）
    this._subscribeToClipboardChanges();

    // 订阅历史记录删除事件（用于重置 remoteContent 的 fileUri）
    this._subscribeToHistoryChanges();

    // 订阅 localPollingInterval 配置变化（更新 ClipboardMonitor 轮询间隔）
    this._subscribeToLocalPollingIntervalChanges();

    // 订阅传输队列状态变化（同步到 store，供 UI 展示）
    this._subscribeToTransferQueue();

    // 订阅 AppState 变化（前台/后台切换时管理连接状态）
    this._subscribeToAppState();
  }

  /**
   * 停止同步服务。
   */
  async stop(): Promise<void> {
    this._isStarted = false;
    this._unsubscribeFromClipboardChanges();
    this._unsubscribeFromHistoryChanges();
    this._unsubscribeFromLocalPollingIntervalChanges();
    this._unsubscribeFromTransferQueue();
    this._unsubscribeFromAppState();
    await this._stopConnection();
    await this._destroySyncManager();

    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    useClipboardSyncServiceStore.getState().setRemoteContent(null);

    this.activeServer = null;
    this.lastRemoteProfileHash = null;
    this.lastLocalProfileHash = null;
  }

  /**
   * 刷新服务（配置变更时由 BackgroundServiceManager 调用）。
   * 若服务器变更则完全重启；若服务器未变且连接仍活跃则只确保订阅存在（跳过重连）；
   * 若连接已断开则重建。
   */
  async refresh(): Promise<void> {
    const { useSettingsStore } = require('../stores/settingsStore');
    const newServer = useSettingsStore.getState().getActiveServer();
    const serverChanged = JSON.stringify(newServer) !== JSON.stringify(this.activeServer);

    if (serverChanged) {
      await this.stop();
      await this.start();
    } else if (this.activeServer) {
      // 服务器未变：若连接仍活跃则不重连（避免与 App.tsx start() 双重触发时的无谓断联）
      const connectionActive = !!this.pollingTag || this.signalRConnected;
      if (!connectionActive) {
        await this._startConnection(this.activeServer);
      }
      // 确保订阅存在（幂等）
      this._subscribeToClipboardChanges();
      this._subscribeToHistoryChanges();
      this._subscribeToLocalPollingIntervalChanges();
      this._subscribeToTransferQueue();
      this._subscribeToAppState();
    }
  }

  // ─── AppState 通知 ──────────────────────────────────────────────────────

  private _appStateSub: { remove(): void } | null = null;
  private _lastAppState: string = 'active';

  private _subscribeToAppState(): void {
    if (this._appStateSub) return;
    const { AppState } = require('react-native');
    this._lastAppState = AppState.currentState ?? 'active';
    this._appStateSub = AppState.addEventListener('change', async (nextAppState: string) => {
      if (this._lastAppState.match(/inactive|background/) && nextAppState === 'active') {
        await this.onAppForeground();
      } else if (nextAppState === 'background' || nextAppState === 'inactive') {
        await this.onAppBackground();
      }
      this._lastAppState = nextAppState;
    });
  }

  private _unsubscribeFromAppState(): void {
    this._appStateSub?.remove();
    this._appStateSub = null;
  }

  /** 应用切换到前台时调用 */
  async onAppForeground(): Promise<void> {
    this.isAppActive = true;
    if (!this.activeServer) return;

    if (this.activeServer.type === 'syncclipboard') {
      const { getSignalRClient } = require('signalr-client');
      if (!getSignalRClient().isConnected()) {
        await this._connectSignalR(this.activeServer);
      } else {
        // 已连接，补刷一次
        await this.fetchRemoteClipboard(true);
      }
    } else {
      // 轮询模式：若轮询已停止（进入后台时可能停止），重新启动
      const { useSettingsStore } = require('../stores/settingsStore');
      const config = useSettingsStore.getState().config;
      if (!this.pollingTag) {
        this._startPolling(config?.remotePollingInterval);
      }
      await this.fetchRemoteClipboard(true);
    }
  }

  /** 应用切换到后台时调用 */
  async onAppBackground(): Promise<void> {
    this.isAppActive = false;
    const { useSettingsStore } = require('../stores/settingsStore');
    const config = useSettingsStore.getState().config;
    const bgDownloadEnabled = config?.enableBackgroundTasks && config?.enableBackgroundDownload;

    if (!bgDownloadEnabled) {
      this._stopPolling();
      await this._disconnectSignalR();
    }
  }

  // ─── UI 动作接口 ──────────────────────────────────────────────────────────

  /**
   * 下拉刷新：同时更新本地剪贴板和远程剪贴板内容。
   * 错误通过 useErrorStore 上报，不向上抛出，UI 只需 await 并控制 refreshing 状态。
   */
  async refreshContent(): Promise<void> {
    // 更新本地剪贴板内容
    const { useClipboardStore } = require('../stores/clipboardStore');
    await useClipboardStore.getState().getContent();

    // 如有活跃服务器，刷新远程内容
    if (!this.activeServer) return;
    try {
      await this.fetchRemoteClipboard(false);
    } catch (error) {
      const { useErrorStore } = require('../stores/errorStore');
      const errorMessage = error instanceof Error ? error.message : '刷新失败';
      useErrorStore.getState().setError({ title: '刷新失败', message: errorMessage });
    }
  }

  /**
   * 获取远程剪贴板（下拉刷新或初始加载）。
   * silent=false 时设置 loadingRemote 状态，出错时向上抛出异常供 UI 处理。
   */
  async fetchRemoteClipboard(silent: boolean = false): Promise<void> {
    // 优先使用已知的 activeServer，若服务尚未 start 则从配置中读取
    const server = this.activeServer ?? (await this._readActiveServer());

    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');

    if (!server) {
      useClipboardSyncServiceStore.getState().setRemoteContent(null);
      this.lastRemoteProfileHash = null;
      return;
    }

    if (!silent) {
      useClipboardSyncServiceStore.getState().setLoadingRemote(true);
    }

    try {
      const { createAPIClient } = require('./index');
      const apiClient = createAPIClient(server);
      const profile = await apiClient.getClipboard();

      if (profile) {
        const { profileDtoToContent } = await import('../utils/clipboard');
        const content = profileDtoToContent(profile);
        const currentHash = content.profileHash || content.text || '';
        await this._processRemoteClipboardContent(
          content,
          currentHash,
          profile.hasData,
          apiClient,
          silent ? '' : 'Fetch: '
        );
      } else {
        useClipboardSyncServiceStore.getState().setRemoteContent(null);
        this.lastRemoteProfileHash = null;
      }
    } catch (error) {
      if (!silent) {
        useClipboardSyncServiceStore.getState().setRemoteContent(null);
        this.lastRemoteProfileHash = null;
        throw error; // 非静默模式：交由 UI 层处理错误展示
      }
      console.error('[ClipboardSyncService] Silent fetch failed:', error);
    } finally {
      if (!silent) {
        useClipboardSyncServiceStore.getState().setLoadingRemote(false);
      }
    }
  }

  /**
   * 用户触发的剪贴板上传操作。
   * 上传状态通过 ClipboardSyncServiceStore.uploadingClipboard 更新。
   * 调用方无需传入 signal，服务内部管理取消逻辑。
   */
  async triggerUpload(): Promise<SyncResult> {
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const { useSyncStore } = require('../stores/syncStore');

    if (this._uploadAbortController) {
      this._uploadAbortController.abort();
    }

    this._uploadAbortController = new AbortController();
    useClipboardSyncServiceStore.getState().setUploadingClipboard(true);

    try {
      const result = await useSyncStore
        .getState()
        .sync(SyncDirection.Upload, this._uploadAbortController.signal);
      if (result.success) {
        await this.fetchRemoteClipboard(true).catch(() => {});
      }
      return result;
    } finally {
      this._uploadAbortController = null;
      useClipboardSyncServiceStore.getState().setUploadingClipboard(false);
    }
  }

  /**
   * 取消当前正在进行的剪贴板上传。
   */
  cancelUpload(): void {
    if (this._uploadAbortController) {
      this._uploadAbortController.abort();
      this._uploadAbortController = null;
      const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
      useClipboardSyncServiceStore.getState().setUploadingClipboard(false);
    }
  }

  /**
   * 记录最近操作的本地哈希，防止将其重复自动复制。
   * 手动将远程内容复制到本地剪贴板后调用。
   */
  recordLocalHash(hash: string): void {
    this.lastLocalProfileHash = hash;
  }

  /** 后台 SignalR 是否连接中（供 HomeScreen 断开 SignalR 时判断是否可以真正 disconnect）。 */
  isSignalRRunning(): boolean {
    return this.signalRConnected;
  }

  // ─── 私有实现 ─────────────────────────────────────────────────────────────

  private async _readActiveServer(): Promise<ServerConfig | null> {
    const { useSettingsStore } = require('../stores/settingsStore');
    if (!useSettingsStore.getState().isLoaded) {
      await useSettingsStore.getState().loadConfig();
    }
    return useSettingsStore.getState().getActiveServer();
  }

  private async _initializeSyncManager(): Promise<void> {
    try {
      const { useSyncStore } = require('../stores/syncStore');
      await useSyncStore.getState().initialize();
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to initialize SyncManager:', e);
    }
  }

  private async _destroySyncManager(): Promise<void> {
    try {
      const { useSyncStore } = require('../stores/syncStore');
      await useSyncStore.getState().destroy();
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to destroy SyncManager:', e);
    }
  }

  private async _initializeHistorySyncService(server: ServerConfig): Promise<void> {
    try {
      const { useSettingsStore } = require('../stores/settingsStore');
      const config = useSettingsStore.getState().config;
      if (config?.enableHistorySync) {
        const { getHistorySyncService } = require('./HistorySyncService');
        await getHistorySyncService().ensureInitialized(server);
        console.log('[ClipboardSyncService] HistorySyncService initialized');
      }
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to initialize HistorySyncService:', e);
    }
  }

  private async _startConnection(server: ServerConfig): Promise<void> {
    const { useSettingsStore } = require('../stores/settingsStore');
    const config = useSettingsStore.getState().config;

    if (server.type === 'syncclipboard') {
      await this._connectSignalR(server);
    } else {
      this._startPolling(config?.remotePollingInterval);
      // 立即获取一次（非静默，显示加载状态；错误写入 errorStore）
      await this.fetchRemoteClipboard(false).catch((error: Error) => {
        const { useErrorStore } = require('../stores/errorStore');
        const errorMessage = error?.message ?? '无法连接到服务器';
        useErrorStore.getState().setError({ title: '连接失败', message: errorMessage });
      });
    }
  }

  private async _stopConnection(): Promise<void> {
    this._stopPolling();
    await this._disconnectSignalR();
  }

  private _startPolling(interval?: number): void {
    if (this.pollingTag) return;
    try {
      const { setTimer } = require('native-timer');
      const pollingInterval = interval ?? 3000;
      this.pollingTag = setTimer(
        () => {
          this.fetchRemoteClipboard(true).catch(() => {});
        },
        pollingInterval,
        'remote_sync_poll'
      );
      console.log('[ClipboardSyncService] Polling started, interval:', pollingInterval);
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to start polling:', e);
    }
  }

  private _stopPolling(): void {
    if (this.pollingTag) {
      try {
        const { clearTimer } = require('native-timer');
        clearTimer(this.pollingTag);
      } catch {}
      this.pollingTag = null;
    }
  }

  private async _connectSignalR(server: ServerConfig): Promise<void> {
    if (this.signalRConnected) return;
    try {
      const { getSignalRClient } = require('signalr-client');
      const client = getSignalRClient();
      client.onRemoteClipboardChanged(this._signalRCallback);
      await client.connect(server);
      this.signalRConnected = true;
      console.log('[ClipboardSyncService] SignalR connected');
      // 连接后立即获取一次（非静默；错误写入 errorStore）
      await this.fetchRemoteClipboard(false).catch((error: Error) => {
        const { useErrorStore } = require('../stores/errorStore');
        const errorMessage = error?.message ?? '无法连接到服务器';
        useErrorStore.getState().setError({ title: '连接失败', message: errorMessage });
      });
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to connect SignalR:', e);
    }
  }

  private async _disconnectSignalR(): Promise<void> {
    if (!this.signalRConnected) return;
    this.signalRConnected = false;
    try {
      const { getSignalRClient } = require('signalr-client');
      const client = getSignalRClient();
      client.offRemoteClipboardChanged(this._signalRCallback);
      await client.disconnect();
      console.log('[ClipboardSyncService] SignalR disconnected');
    } catch {}
  }

  /**
   * 处理远程剪贴板内容变化的核心逻辑。
   * 包含哈希检测、自动下载、历史记录写入、自动复制等所有业务规则。
   */
  private async _processRemoteClipboardContent(
    content: ClipboardContent,
    currentHash: string,
    hasData: boolean,
    apiClient: ISyncClipboardAPI,
    logPrefix: string = ''
  ): Promise<void> {
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const { useSettingsStore } = require('../stores/settingsStore');
    const { historyStorage } = require('./HistoryStorage');
    const config = useSettingsStore.getState().config;
    const previousHash = this.lastRemoteProfileHash;

    const { resolveRemoteContent } = await import('../utils/processRemoteContent');
    const { SyncManager } = require('./SyncManager');
    const resolved = await resolveRemoteContent(content, currentHash, previousHash, hasData, {
      getLastUploadedHash: () => SyncManager.getInstance().getLastUploadedHash(),
      getHistoryItem: (profileHash: string) => historyStorage.getItem(profileHash),
      getHistoryFileUri: async (type: string, profileHash: string, fileName: string) => {
        const { getHistoryFileUri } = await import('../utils/fileStorage');
        return getHistoryFileUri(type, profileHash, fileName);
      },
    });

    // 没有变化，不处理
    if (!resolved) return;

    // fileUri 更新（后台已下载文件），只更新显示
    if (resolved.fileUriOnlyUpdate) {
      const store = useClipboardSyncServiceStore.getState();
      const prev = store.remoteContent;
      if (prev?.fileUri !== resolved.content.fileUri) {
        store.setRemoteContent(
          prev ? { ...prev, fileUri: resolved.content.fileUri } : resolved.content
        );
      }
      return;
    }

    // 是本地刚上传的内容，跳过自动下载/复制，仅更新显示
    if (resolved.isJustUploaded) {
      console.log(
        `[ClipboardSyncService] ${logPrefix}Remote hash matches last uploaded hash, skipping auto-download/copy`
      );
      this.lastRemoteProfileHash = currentHash;
      useClipboardSyncServiceStore.getState().setRemoteContent(resolved.content);
      return;
    }

    this.lastRemoteProfileHash = currentHash;

    let finalContent = resolved.content;
    let skipAutoCopyDueToLargeFile = false;
    const foundInHistory = resolved.foundInHistory;

    if (foundInHistory) {
      console.log(
        `[ClipboardSyncService] ${logPrefix}Found existing file in history, skipping download`
      );
    }

    // 自动下载（文件大小在限制内时）
    if (!foundInHistory) {
      const autoDownloadMaxSize = config?.autoDownloadMaxSize ?? 5 * 1024 * 1024;
      const hasFileData = hasData && content.fileName && content.fileSize !== undefined;

      if (hasFileData) {
        const fileTooLarge = content.fileSize! > autoDownloadMaxSize;
        if (fileTooLarge) {
          skipAutoCopyDueToLargeFile = true;
          console.log(
            `[ClipboardSyncService] ${logPrefix}File too large (${content.fileSize} > ${autoDownloadMaxSize}), skipping auto-download`
          );
        } else {
          try {
            const { downloadAndAddToHistory } = await import('../utils/remoteClipboard');
            finalContent = await downloadAndAddToHistory(content, apiClient, hasData);
            console.log(`[ClipboardSyncService] ${logPrefix}Auto-download completed`);
          } catch (downloadError) {
            console.error(
              `[ClipboardSyncService] ${logPrefix}Auto-download failed:`,
              downloadError
            );
            skipAutoCopyDueToLargeFile = true;
          }
        }
      }
    }

    // 更新 UI 显示
    useClipboardSyncServiceStore.getState().setRemoteContent(finalContent);

    const isFirstLoad = previousHash === null;
    if (!isFirstLoad) {
      // 没有文件数据时，添加不含文件的历史记录
      // （有文件数据的情况已由 downloadAndAddToHistory 处理）
      if (!hasData) {
        try {
          const { useHistoryStore } = require('../stores/historyStore');
          const historyItem = createDefaultClipboardItem({
            type: finalContent.type,
            text: finalContent.text || '',
            profileHash: finalContent.profileHash || '',
            hasData: false,
            dataName: finalContent.fileName,
            size: finalContent.fileSize,
            timestamp: finalContent.timestamp || Date.now(),
            syncStatus: HistorySyncStatus.Synced,
          });
          await useHistoryStore.getState().addItem(historyItem);
          console.log(
            `[ClipboardSyncService] ${logPrefix}Added remote clipboard (no file) to history`
          );
        } catch (error) {
          console.error(`[ClipboardSyncService] ${logPrefix}Failed to add to history:`, error);
        }
      }

      // 自动复制：autoSync 开启 或 后台下载启用时
      const autoSyncEnabled = config?.autoSync ?? false;
      const bgDownloadEnabled = !!(
        config?.enableBackgroundTasks && config?.enableBackgroundDownload
      );
      const shouldAutoCopy = autoSyncEnabled || (!this.isAppActive && bgDownloadEnabled);
      const remoteHash = finalContent.profileHash || finalContent.text || '';
      const localMatchesRemote = remoteHash === this.lastLocalProfileHash;

      if (
        !localMatchesRemote &&
        shouldAutoCopy &&
        this.activeServer &&
        !this.isAutoSyncing &&
        !skipAutoCopyDueToLargeFile &&
        finalContent.type === 'Text'
      ) {
        this.isAutoSyncing = true;
        try {
          const result = await this._copyToLocalClipboard(finalContent, logPrefix);
          if (result.success && Platform.OS === 'android') {
            const preview =
              finalContent.type === 'Text' && finalContent.text
                ? finalContent.text.trim().replace(/\s+/g, ' ').slice(0, 30)
                : finalContent.fileName || finalContent.type;
            SyncManager.getInstance().updateForegroundNotification(`已下载: ${preview}`);
            if (config?.syncToastEnabled !== false) {
              ToastAndroid.show(`已下载\n${preview}`, ToastAndroid.SHORT);
            }
          }
        } catch (error) {
          console.error(`[ClipboardSyncService] ${logPrefix}Auto-copy failed:`, error);
        } finally {
          this.isAutoSyncing = false;
        }
      }
    }
  }

  /**
   * 将内容复制到本地剪贴板，并记录哈希以防止重复自动复制。
   */
  private async _copyToLocalClipboard(
    content: ClipboardContent,
    logPrefix: string = ''
  ): Promise<{ success: boolean; message?: string }> {
    const { copyToLocalClipboard } = await import('../utils/clipboard');
    const result = await copyToLocalClipboard(content);
    if (result.success) {
      const { useClipboardStore } = require('../stores/clipboardStore');
      useClipboardStore.getState().setCurrentContentDisplay(content);
      this.lastLocalProfileHash = content.profileHash || content.text || '';
      console.log(`[ClipboardSyncService] ${logPrefix}Copied to local clipboard`);
    } else {
      console.error(`[ClipboardSyncService] ${logPrefix}Copy failed: ${result.message}`);
    }
    return result;
  }

  private _subscribeToTransferQueue(): void {
    if (this.transferQueueHandler) return;
    const { getHistoryTransferQueue } = require('./HistoryTransferQueue');
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const { getProfileId } = require('./HistoryAPI');
    const queue = getHistoryTransferQueue();

    const handler = async (task: import('./HistoryTransferQueue').TransferTask) => {
      if (task.type !== 'download') return;

      const currentRemote = useClipboardSyncServiceStore.getState().remoteContent;
      if (!currentRemote?.profileHash) return;

      const profileId = getProfileId(currentRemote.type, currentRemote.profileHash);
      if (task.profileId !== profileId) return;

      const store = useClipboardSyncServiceStore.getState();

      if (
        task.status === 'running' ||
        task.status === 'pending' ||
        task.status === 'waitForRetry'
      ) {
        // 任务进行中：更新下载状态和进度
        store.setDownloadingRemote(true);
        if (task.status === 'running' && task.progress >= 0) {
          store.setDownloadProgress({
            progress: task.progress / 100,
            bytesTransferred: task.bytesTransferred,
            totalBytes: task.totalBytes || 0,
          });
        }
      } else if (task.status === 'completed') {
        // 下载完成：更新 remoteContent 的 fileUri 并清除下载状态
        const { getHistoryFileUri } = require('../utils/fileStorage');
        const fileUri = await getHistoryFileUri(
          currentRemote.type,
          currentRemote.profileHash,
          currentRemote.fileName!
        );
        if (fileUri && fileUri !== currentRemote.fileUri) {
          store.setRemoteContent({ ...currentRemote, fileUri });
          const { useSettingsStore } = require('../stores/settingsStore');
          const config = useSettingsStore.getState().config;
          if (config?.syncToastEnabled !== false) {
            ToastAndroid.show('文件已下载', ToastAndroid.SHORT);
          }
        }
        store.setDownloadingRemote(false);
        store.setDownloadProgress(null);
      } else {
        // 失败或取消：清除下载状态
        store.setDownloadingRemote(false);
        store.setDownloadProgress(null);
      }
    };

    this.transferQueueHandler = handler;
    queue.onTaskStatusChanged(handler);
  }

  private _unsubscribeFromTransferQueue(): void {
    if (!this.transferQueueHandler) return;
    const { getHistoryTransferQueue } = require('./HistoryTransferQueue');
    const queue = getHistoryTransferQueue();
    queue.offTaskStatusChanged(this.transferQueueHandler);
    this.transferQueueHandler = null;
    // 清除下载状态
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const store = useClipboardSyncServiceStore.getState();
    store.setDownloadingRemote(false);
    store.setDownloadProgress(null);
  }

  private _subscribeToClipboardChanges(): void {
    if (this.clipboardUnsub) return;
    const { useClipboardStore } = require('../stores/clipboardStore');
    this.clipboardUnsub = useClipboardStore.subscribe(
      (state: { currentContent: unknown }, prevState: { currentContent: unknown }) => {
        if (state.currentContent !== prevState.currentContent && state.currentContent) {
          this._handleAutoUpload(state.currentContent as ClipboardContent);
        }
      }
    );
  }

  private _unsubscribeFromClipboardChanges(): void {
    this.clipboardUnsub?.();
    this.clipboardUnsub = null;
  }

  /**
   * 订阅历史记录删除事件，当 remoteContent 对应的条目被删除时重置其 fileUri。
   */
  private _subscribeToHistoryChanges(): void {
    if (this.historyUnsub) return;
    const { useHistoryStore } = require('../stores/historyStore');
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');

    this.historyUnsub = useHistoryStore.subscribe(
      (
        state: { lastDeletedHashes: string[]; historyCleared: boolean },
        prevState: { lastDeletedHashes: string[]; historyCleared: boolean }
      ) => {
        const hasNewDeletions =
          state.lastDeletedHashes !== prevState.lastDeletedHashes ||
          state.historyCleared !== prevState.historyCleared;
        if (!hasNewDeletions) return;

        const { historyCleared, lastDeletedHashes } = state;
        if (!historyCleared && lastDeletedHashes.length === 0) return;

        const currentRemote = useClipboardSyncServiceStore.getState().remoteContent;
        if (!currentRemote?.profileHash) {
          useHistoryStore.getState().clearDeletedState();
          return;
        }

        if (historyCleared) {
          console.log('[ClipboardSyncService] History cleared, resetting remote content fileUri');
          useClipboardSyncServiceStore
            .getState()
            .setRemoteContent({ ...currentRemote, fileUri: undefined });
          useHistoryStore.getState().clearDeletedState();
          return;
        }

        if (lastDeletedHashes.length > 0) {
          const deletedSet = new Set(lastDeletedHashes.map((h: string) => h.toLowerCase()));
          if (deletedSet.has(currentRemote.profileHash.toLowerCase())) {
            console.log(
              '[ClipboardSyncService] Remote content deleted from history, resetting fileUri:',
              currentRemote.profileHash
            );
            useClipboardSyncServiceStore
              .getState()
              .setRemoteContent({ ...currentRemote, fileUri: undefined });
          }
          useHistoryStore.getState().clearDeletedState();
        }
      }
    );
  }

  private _unsubscribeFromHistoryChanges(): void {
    this.historyUnsub?.();
    this.historyUnsub = null;
  }

  /**
   * 订阅 localPollingInterval 配置变化，同步更新 ClipboardMonitor 的本地轮询间隔。
   * 初始化时立即应用一次当前值。
   */
  private _localPollingIntervalUnsub: (() => void) | null = null;

  private _subscribeToLocalPollingIntervalChanges(): void {
    if (this._localPollingIntervalUnsub) return;
    const { useSettingsStore } = require('../stores/settingsStore');
    const { useClipboardStore } = require('../stores/clipboardStore');

    // 立即应用当前值
    const currentInterval = useSettingsStore.getState().config?.localPollingInterval ?? 1000;
    useClipboardStore.getState().updatePollingInterval(currentInterval);

    this._localPollingIntervalUnsub = useSettingsStore.subscribe(
      (
        state: { config: { localPollingInterval?: number } | null | undefined },
        prevState: { config: { localPollingInterval?: number } | null | undefined }
      ) => {
        const interval = state.config?.localPollingInterval ?? 1000;
        const prevInterval = prevState.config?.localPollingInterval ?? 1000;
        if (interval !== prevInterval) {
          useClipboardStore.getState().updatePollingInterval(interval);
        }
      }
    );
  }

  private _unsubscribeFromLocalPollingIntervalChanges(): void {
    this._localPollingIntervalUnsub?.();
    this._localPollingIntervalUnsub = null;
  }

  /**
   * 本地剪贴板内容变化时触发自动上传。
   * 条件：autoSync 开启 或 后台上传启用。
   */
  private _handleAutoUpload(content: ClipboardContent): void {
    const { useSettingsStore } = require('../stores/settingsStore');
    const config = useSettingsStore.getState().config;

    const autoSync = config?.autoSync ?? false;
    const bgUpload = config?.enableBackgroundTasks && config?.enableBackgroundUpload;
    if (!autoSync && !bgUpload) return;
    if (!this.activeServer) return;

    const currentHash = content.profileHash || content.text || '';

    // 初始化时记录哈希，不触发上传
    if (this.lastLocalProfileHash === null) {
      this.lastLocalProfileHash = currentHash;
      return;
    }

    if (currentHash === this.lastLocalProfileHash) return;
    this.lastLocalProfileHash = currentHash;

    if (this.isAutoSyncing) return;
    this.isAutoSyncing = true;

    const { useSyncStore } = require('../stores/syncStore');
    useSyncStore
      .getState()
      .sync(SyncDirection.Upload)
      .then((result: SyncResult) => {
        if (result.success && !result.skipped && Platform.OS === 'android') {
          const preview =
            content.type === 'Text' && content.text
              ? content.text.trim().replace(/\s+/g, ' ').slice(0, 30)
              : content.fileName || content.type;
          const { SyncManager } = require('./SyncManager');
          SyncManager.getInstance().updateForegroundNotification(`已上传: ${preview}`);
          if (config?.syncToastEnabled !== false) {
            ToastAndroid.show(`已上传\n${preview}`, ToastAndroid.SHORT);
          }
          // 上传成功后静默刷新远程显示
          this.fetchRemoteClipboard(true).catch(() => {});
        }
      })
      .catch((e: Error) => console.error('[ClipboardSyncService] Auto-upload failed:', e))
      .finally(() => {
        this.isAutoSyncing = false;
      });
  }

  // ─── 用户触发的文件下载操作 ──────────────────────────────────

  /**
   * 下载当前远程剪贴板的文件数据到本地。
   * - WebDAV/S3/自定义服务器：直接下载文件，进度通过 store 更新
   * - SyncClipboard 服务器：加入下载队列
   * 调用方无需传入 activeServer，服务内部从 store 读取。
   */
  async downloadRemoteFile(): Promise<void> {
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const remoteContent = useClipboardSyncServiceStore.getState().remoteContent;
    const server = this.activeServer;

    if (!server || !remoteContent) return;

    if (server.type !== 'syncclipboard') {
      await this._downloadForWebDAV(server, remoteContent);
    } else {
      await this._downloadForSyncClipboard(server, remoteContent);
    }
  }

  /**
   * 取消当前正在进行的远程文件下载。
   */
  cancelRemoteFileDownload(): void {
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const remoteContent = useClipboardSyncServiceStore.getState().remoteContent;
    const server = this.activeServer;

    if (!server) return;

    if (server.type !== 'syncclipboard') {
      if (this._downloadAbortController) {
        this._downloadAbortController.abort();
        this._downloadAbortController = null;
      }
    } else {
      if (remoteContent?.profileHash) {
        const { getProfileId } = require('../utils/clipboard');
        const { getHistoryTransferQueue } = require('./HistoryTransferQueue');
        const profileId = getProfileId(remoteContent.type, remoteContent.profileHash);
        getHistoryTransferQueue().cancelTask(profileId, 'download');
      }
    }
  }

  /**
   * 上传文件到服务器（由 HomeScreen 在用户选择文件/图片后触发）。
   * 上传进度通过 ClipboardSyncServiceStore.fileUploadProgress 更新。
   * 调用方无需传入 activeServer，服务内部从 store 读取。
   */
  async uploadFile(
    payload: { uri: string; fileName: string; mimeType?: string | null; fileSize?: number },
    signal: AbortSignal
  ): Promise<void> {
    const server = this.activeServer;
    if (!server) throw new Error('请先在设置中配置服务器');

    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const store = useClipboardSyncServiceStore.getState();
    store.setFileUploadProgress({ stage: '正在处理文件…', progress: null });

    try {
      const { uploadFileAndAddToHistory } = await import('../utils/uploadFile');
      await uploadFileAndAddToHistory(
        payload.uri,
        payload.fileName,
        payload.mimeType,
        payload.fileSize,
        server,
        {
          signal,
          onProgress: (stage: string, progressInfo?: import('native-util').ProgressInfo) => {
            useClipboardSyncServiceStore
              .getState()
              .setFileUploadProgress({ stage, progressInfo: progressInfo ?? null });
          },
        }
      );
    } finally {
      useClipboardSyncServiceStore.getState().setFileUploadProgress(null);
    }
  }

  /** WebDAV/S3 直接下载，进度通过 store 更新 */
  private async _downloadForWebDAV(
    server: ServerConfig,
    remoteContent: import('../types/clipboard').ClipboardContent
  ): Promise<void> {
    const { useClipboardSyncServiceStore } = require('../stores/ClipboardSyncServiceStore');
    const store = useClipboardSyncServiceStore.getState();

    store.setDownloadingRemote(true);
    store.setDownloadProgress(null);

    const abortController = new AbortController();
    this._downloadAbortController = abortController;

    try {
      const { createAPIClient } = require('./APIClient');
      const { downloadAndAddToHistory } = await import('../utils/remoteClipboard');

      const apiClient = createAPIClient(server);
      const updatedContent = await downloadAndAddToHistory(
        remoteContent,
        apiClient,
        remoteContent.hasData || false,
        abortController.signal,
        (info: import('native-util').ProgressInfo) => {
          useClipboardSyncServiceStore.getState().setDownloadProgress({
            progress: info.progress,
            bytesTransferred: info.bytesTransferred,
            totalBytes: info.totalBytes,
          });
        }
      );
      useClipboardSyncServiceStore.getState().setRemoteContent(updatedContent);
      return;
    } catch (error) {
      const err = error as Error;
      const msg = err?.message?.toLowerCase() ?? '';
      if (err?.name === 'AbortError' || msg.includes('abort') || msg.includes('cancel')) {
        // 取消不再向上抛出，由 UI 层通过 store 感知（downloadingRemote = false）
        return;
      }
      throw error;
    } finally {
      this._downloadAbortController = null;
      useClipboardSyncServiceStore.getState().setDownloadingRemote(false);
      useClipboardSyncServiceStore.getState().setDownloadProgress(null);
    }
  }

  /** SyncClipboard 服务器：加入下载队列 */
  private async _downloadForSyncClipboard(
    _server: ServerConfig,
    remoteContent: import('../types/clipboard').ClipboardContent
  ): Promise<void> {
    if (!remoteContent.profileHash) return;

    const { getProfileId } = require('./HistoryAPI');
    const { createDefaultClipboardItem } = require('../types/clipboard');
    const { HistorySyncStatus } = require('../types/clipboard');
    const { getHistoryTransferQueue } = require('./HistoryTransferQueue');
    const { useHistoryStore } = require('../stores/historyStore');

    const profileId = getProfileId(remoteContent.type, remoteContent.profileHash);
    const queue = getHistoryTransferQueue();

    try {
      const historyItem = createDefaultClipboardItem({
        type: remoteContent.type,
        text: remoteContent.text || '',
        profileHash: remoteContent.profileHash,
        hasData: remoteContent.hasData || false,
        dataName: remoteContent.fileName,
        size: remoteContent.fileSize,
        timestamp: remoteContent.timestamp || Date.now(),
        syncStatus: HistorySyncStatus.NeedSync,
        hasRemoteData: true,
        isLocalFileReady: false,
      });
      await useHistoryStore.getState().addItem(historyItem);
    } catch (e) {
      console.error('[ClipboardSyncService] Failed to add history item before download:', e);
    }

    await queue.addDownloadTask(profileId, true);
  }
}

export function getClipboardSyncService(): ClipboardSyncService {
  return ClipboardSyncService.getInstance();
}
