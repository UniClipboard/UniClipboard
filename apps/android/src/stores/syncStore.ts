/**
 * Sync Store
 * 同步状态管理 - 使用 Zustand
 */

import { create } from 'zustand';
import {
  SyncStatus,
  SyncMode,
  SyncDirection,
  SyncResult,
  SyncStats,
  SyncEvent,
  SyncEventType,
} from '../types/sync';
import { SyncManager } from '../services';
import { configStorage } from '../services';

/**
 * 同步状态接口
 */
interface SyncState {
  // 状态
  /** 同步管理器实例 */
  manager: SyncManager | null;

  /** 同步状态 */
  status: SyncStatus;

  /** 同步模式 */
  mode: SyncMode;

  /** 是否已初始化 */
  isInitialized: boolean;

  /** 最后同步结果 */
  lastResult: SyncResult | null;

  /** 同步统计 */
  stats: SyncStats | null;

  /** 离线队列大小 */
  offlineQueueSize: number;

  /** 错误信息 */
  error: string | null;

  // 动作
  /** 初始化同步管理器 */
  initialize: () => Promise<void>;

  /** 执行同步 */
  sync: (direction?: SyncDirection, signal?: AbortSignal) => Promise<SyncResult>;

  /** 更新同步模式 */
  setSyncMode: (mode: SyncMode) => Promise<void>;

  /** 更新同步间隔 */
  setSyncInterval: (interval: number) => Promise<void>;

  /** 刷新统计信息 */
  refreshStats: () => void;

  /** 清空离线队列 */
  clearOfflineQueue: () => Promise<void>;

  /** 清除错误 */
  clearError: () => void;

  /** 销毁 */
  destroy: () => Promise<void>;
}

/**
 * 初始状态
 */
const initialState = {
  manager: null,
  status: SyncStatus.Idle,
  mode: SyncMode.Manual,
  isInitialized: false,
  lastResult: null,
  stats: null,
  offlineQueueSize: 0,
  error: null,
};

/**
 * 创建同步 Store
 */
export const useSyncStore = create<SyncState>((set, get) => ({
  ...initialState,

  initialize: async () => {
    if (get().isInitialized) {
      return;
    }

    try {
      const manager = SyncManager.getInstance();
      const config = await configStorage.getConfig();

      // 获取激活的服务器配置
      const activeServer = await configStorage.getActiveServer();

      if (!activeServer) {
        set({
          error: 'No active server configured',
          isInitialized: false,
        });
        return;
      }

      // 初始化同步管理器
      await manager.initialize({
        server: activeServer,
        mode: config.syncMode as SyncMode,
        interval: config.syncInterval,
        conflictResolution: config.conflictResolution,
        enableOfflineQueue: config.enableOfflineQueue,
        maxOfflineQueueSize: config.maxOfflineQueueSize,
        syncLargeFiles: config.syncLargeFiles,
        largeFileThreshold: config.largeFileThreshold,
        maxRetries: 3,
        retryDelay: 2000,
      });

      // 添加事件监听器
      manager.addListener('store', (event: SyncEvent) => {
        switch (event.type) {
          case SyncEventType.StatusChanged:
            if (event.status) {
              set({ status: event.status });
            }
            break;

          case SyncEventType.Completed:
            if (event.result) {
              set({
                lastResult: event.result,
                stats: manager.getStats(),
                offlineQueueSize: manager.getOfflineQueueSize(),
                error: null,
              });
            }
            break;

          case SyncEventType.Failed:
            if (event.result) {
              set({
                lastResult: event.result,
                error: event.result.error || 'Sync failed',
                stats: manager.getStats(),
              });
            }
            break;

          case SyncEventType.Conflict:
            set({
              error: 'Sync conflict detected',
            });
            break;
        }
      });

      set({
        manager,
        mode: config.syncMode as SyncMode,
        stats: manager.getStats(),
        offlineQueueSize: manager.getOfflineQueueSize(),
        isInitialized: true,
        error: null,
      });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to initialize sync';
      set({ error: errorMessage, isInitialized: false });
    }
  },

  sync: async (direction = SyncDirection.Both, signal?: AbortSignal) => {
    const { manager, isInitialized } = get();

    if (!isInitialized || !manager) {
      const error = 'Sync manager not initialized';
      set({ error });
      return {
        success: false,
        direction,
        error,
      };
    }

    set({ error: null });

    try {
      const result = await manager.sync(direction, false, signal);

      set({
        lastResult: result,
        stats: manager.getStats(),
        offlineQueueSize: manager.getOfflineQueueSize(),
        error: result.success ? null : result.error || 'Sync failed',
      });

      return result;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Sync failed';
      set({ error: errorMessage });

      return {
        success: false,
        direction,
        error: errorMessage,
      };
    }
  },

  setSyncMode: async (mode: SyncMode) => {
    const { manager } = get();

    try {
      // 更新配置存储
      await configStorage.updateConfig({ syncMode: mode as SyncMode });

      // 更新同步管理器配置
      if (manager) {
        await manager.updateConfig({ mode });
      }

      set({ mode, error: null });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to update sync mode';
      set({ error: errorMessage });
    }
  },

  setSyncInterval: async (interval: number) => {
    const { manager } = get();

    try {
      // 更新配置存储
      await configStorage.updateConfig({ syncInterval: interval });

      // 更新同步管理器配置
      if (manager) {
        await manager.updateConfig({ interval });
      }

      set({ error: null });
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : 'Failed to update sync interval';
      set({ error: errorMessage });
    }
  },

  refreshStats: () => {
    const { manager } = get();

    if (manager) {
      set({
        stats: manager.getStats(),
        status: manager.getStatus(),
        offlineQueueSize: manager.getOfflineQueueSize(),
      });
    }
  },

  clearOfflineQueue: async () => {
    const { manager } = get();

    if (manager) {
      try {
        await manager.clearOfflineQueue();
        set({ offlineQueueSize: 0, error: null });
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : 'Failed to clear offline queue';
        set({ error: errorMessage });
      }
    }
  },

  clearError: () => {
    set({ error: null });
  },

  destroy: async () => {
    const { manager } = get();

    if (manager) {
      manager.removeListener('store');
      await manager.destroy();
    }

    set(initialState);
  },
}));
