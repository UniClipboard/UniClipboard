/**
 * History Storage Service
 * 历史记录存储服务 - 管理剪贴板历史记录
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { ClipboardItem, HistorySyncStatus } from '../types/clipboard';
import { HistoryFilter, HistorySort, STORAGE_KEYS } from '../types/storage';
import { getHistoryFileDir } from '../utils/fileStorage';
import { File, Directory } from 'expo-file-system';

/**
 * 当前历史记录数据版本号
 * 每次数据结构变更时递增
 */
const CURRENT_HISTORY_VERSION = 1;

/**
 * 迁移函数类型
 */
type MigrationFunction = (items: ClipboardItem[]) => ClipboardItem[];

/**
 * 版本迁移函数映射
 * key: 目标版本号
 * value: 从上一版本迁移到目标版本的函数
 */
const MIGRATIONS: Record<number, MigrationFunction> = {
  // v0 -> v1: 添加 syncStatus, isLocalFileReady, lastAccessed 字段
  1: (items: ClipboardItem[]): ClipboardItem[] => {
    return items.map((item) => {
      const migratedItem = { ...item };

      // 设置同步状态默认值
      if (migratedItem.syncStatus === undefined) {
        if (migratedItem.fileUri || !migratedItem.hasData) {
          migratedItem.isLocalFileReady = true;
        }
        migratedItem.syncStatus = HistorySyncStatus.LocalOnly;
      }

      // 设置 isLocalFileReady 默认值
      if (migratedItem.isLocalFileReady === undefined) {
        migratedItem.isLocalFileReady = !!(migratedItem.fileUri || !migratedItem.hasData);
      }

      // 设置 lastAccessed 默认值
      if (migratedItem.lastAccessed === undefined) {
        migratedItem.lastAccessed = migratedItem.timestamp;
      }

      return migratedItem;
    });
  },
};

/**
 * 历史记录存储服务
 */
export type HistoryChangeCallback = (
  items: ClipboardItem[],
  action: 'add' | 'update' | 'delete'
) => void;

/**
 * 规范化 ClipboardItem，确保所有字段都有默认值
 */
function normalizeClipboardItem(item: ClipboardItem): ClipboardItem {
  return {
    type: item.type,
    text: item.text ?? '',
    profileHash: item.profileHash,
    hasData: item.hasData ?? false,
    dataName: item.dataName,
    size: item.size ?? 0,
    timestamp: item.timestamp ?? Date.now(),
    deviceName: item.deviceName,
    synced: item.synced,
    starred: item.starred ?? false,
    useCount: item.useCount ?? 0,
    localClipboardHash: item.localClipboardHash,
    fileUri: item.fileUri,
    syncStatus: item.syncStatus ?? HistorySyncStatus.LocalOnly,
    version: item.version ?? 0,
    lastModified: item.lastModified ?? Date.now(),
    lastAccessed: item.lastAccessed ?? Date.now(),
    isDeleted: item.isDeleted ?? false,
    pinned: item.pinned ?? false,
    isLocalFileReady: item.isLocalFileReady ?? true,
    from: item.from,
    hasRemoteData: item.hasRemoteData ?? false,
  };
}

export class HistoryStorage {
  private static instance: HistoryStorage | null = null;
  private history: ClipboardItem[] = [];
  private initialized = false;
  private maxHistorySize = 1000;
  private changeCallbacks: Set<HistoryChangeCallback> = new Set();
  private pendingChanges: { items: ClipboardItem[]; action: 'add' | 'update' | 'delete' }[] = [];
  private notifyTimer: NodeJS.Timeout | null = null;
  private static readonly NOTIFY_BATCH_SIZE = 50;
  private static readonly NOTIFY_DELAY_MS = 100;
  private silentMode = false;
  private sortConfig: HistorySort = { field: 'timestamp', order: 'desc' };

  private constructor() {}

  /**
   * 获取单例实例
   */
  public static getInstance(): HistoryStorage {
    if (!HistoryStorage.instance) {
      HistoryStorage.instance = new HistoryStorage();
    }
    return HistoryStorage.instance;
  }

  /**
   * 设置排序配置，内部数组将立即重排
   */
  public setSortConfig(sort: HistorySort): void {
    this.sortConfig = sort;
    this.sortHistory();
  }

  /**
   * 获取当前排序配置
   */
  public getSortConfig(): HistorySort {
    return { ...this.sortConfig };
  }

  /**
   * 获取排序字段的值
   */
  private getSortValue(item: ClipboardItem): number {
    switch (this.sortConfig.field) {
      case 'timestamp':
        return item.timestamp;
      case 'lastAccessed':
        return item.lastAccessed || item.timestamp;
      case 'useCount':
        return item.useCount || 0;
      case 'size':
        return item.size || 0;
      default:
        return item.timestamp;
    }
  }

  /**
   * 二分查找插入位置（参照桌面端 InsertHistoryInOrder 实现）
   * pinned 项始终排在非 pinned 项之前
   */
  private findInsertIndex(item: ClipboardItem): number {
    const isDesc = this.sortConfig.order === 'desc';
    const isPinned = item.pinned;
    let searchStart = 0;
    let searchEnd = this.history.length;

    if (isPinned) {
      // pinned 只在 pinned 区域内查找
      searchEnd = this.history.findIndex((i) => !i.pinned);
      if (searchEnd === -1) searchEnd = this.history.length;
    } else {
      // 非 pinned 从第一个非 pinned 开始
      searchStart = this.history.findIndex((i) => !i.pinned);
      if (searchStart === -1) searchStart = this.history.length;
    }

    const targetVal = this.getSortValue(item);
    let low = searchStart;
    let high = searchEnd;

    while (low < high) {
      const mid = (low + high) >> 1;
      const midVal = this.getSortValue(this.history[mid]);
      const shouldGoLeft = isDesc ? midVal <= targetVal : midVal >= targetVal;

      if (shouldGoLeft) {
        high = mid;
      } else {
        low = mid + 1;
      }
    }

    return low;
  }

  /**
   * 检查指定位置的元素是否需要重新定位
   * 考虑 pinned 区域边界和排序字段值
   */
  private shouldReposition(index: number): boolean {
    const item = this.history[index];
    const val = this.getSortValue(item);
    const isDesc = this.sortConfig.order === 'desc';

    if (index > 0) {
      const prev = this.history[index - 1];
      // 检查 pinned 边界：非 pinned 不应在 pinned 前面
      if (!item.pinned && prev.pinned) return false; // 这是正确的位置
      if (item.pinned && !prev.pinned) return true; // pinned 应在前面
      const prevVal = this.getSortValue(prev);
      if (isDesc ? prevVal < val : prevVal > val) return true;
    }

    if (index < this.history.length - 1) {
      const next = this.history[index + 1];
      // 检查 pinned 边界
      if (item.pinned && !next.pinned) return false; // 这是正确的位置
      if (!item.pinned && next.pinned) return true; // 非 pinned 不应在 pinned 前面
      const nextVal = this.getSortValue(next);
      if (isDesc ? nextVal > val : nextVal < val) return true;
    }

    return false;
  }

  /**
   * 对内部数组执行全量排序（pinned 项始终在前）
   */
  private sortHistory(): void {
    const { field, order } = this.sortConfig;
    this.history.sort((a, b) => {
      // pinned 优先
      const aPinned = a.pinned ? 1 : 0;
      const bPinned = b.pinned ? 1 : 0;
      if (aPinned !== bPinned) {
        return bPinned - aPinned;
      }

      let compareResult = 0;
      switch (field) {
        case 'timestamp':
          compareResult = a.timestamp - b.timestamp;
          break;
        case 'lastAccessed':
          compareResult = (a.lastAccessed || a.timestamp) - (b.lastAccessed || b.timestamp);
          break;
        case 'useCount':
          compareResult = (a.useCount || 0) - (b.useCount || 0);
          break;
        case 'size':
          compareResult = (a.size || 0) - (b.size || 0);
          break;
      }
      return order === 'desc' ? -compareResult : compareResult;
    });
  }

  /**
   * 注册变更回调
   */
  public addChangeCallback(callback: HistoryChangeCallback): void {
    this.changeCallbacks.add(callback);
  }

  /**
   * 移除变更回调
   */
  public removeChangeCallback(callback: HistoryChangeCallback): void {
    this.changeCallbacks.delete(callback);
  }

  public beginSilentMode(): void {
    this.silentMode = true;
  }

  public endSilentMode(): void {
    this.silentMode = false;
  }

  private notifyChange(item: ClipboardItem, action: 'add' | 'update' | 'delete'): void {
    if (this.silentMode) {
      return;
    }

    // 浅拷贝，避免 store 中的旧引用和新通知指向同一对象导致比较失效
    this.pendingChanges.push({ items: [{ ...item }], action });

    if (this.pendingChanges.length >= HistoryStorage.NOTIFY_BATCH_SIZE) {
      this.flushPendingChanges();
      return;
    }

    if (!this.notifyTimer) {
      this.notifyTimer = setTimeout(() => {
        this.flushPendingChanges();
      }, HistoryStorage.NOTIFY_DELAY_MS);
    }
  }

  /**
   * 立即批量通知变更
   */
  private notifyChangeBatch(items: ClipboardItem[], action: 'add' | 'update' | 'delete'): void {
    // 浅拷贝，避免 store 中的旧引用和新通知指向同一对象导致比较失效
    const copied = items.map((item) => ({ ...item }));
    for (const callback of this.changeCallbacks) {
      try {
        callback(copied, action);
      } catch (error) {
        console.error('[HistoryStorage] Error in change callback:', error);
      }
    }
  }

  /**
   * 刷新待处理的变更通知
   */
  private flushPendingChanges(): void {
    if (this.notifyTimer) {
      clearTimeout(this.notifyTimer);
      this.notifyTimer = null;
    }

    if (this.pendingChanges.length === 0) return;

    // 按操作类型分组
    const groupedChanges = new Map<'add' | 'update' | 'delete', ClipboardItem[]>();
    for (const change of this.pendingChanges) {
      const existing = groupedChanges.get(change.action) || [];
      existing.push(...change.items);
      groupedChanges.set(change.action, existing);
    }

    // 通知每个分组
    for (const [action, items] of groupedChanges) {
      for (const callback of this.changeCallbacks) {
        try {
          callback(items, action);
        } catch (error) {
          console.error('[HistoryStorage] Error in change callback:', error);
        }
      }
    }

    this.pendingChanges = [];
  }

  /**
   * 初始化历史记录存储
   */
  public async initialize(): Promise<void> {
    if (this.initialized) {
      return;
    }

    try {
      // 从配置中读取最大历史记录条数
      try {
        const { configStorage } = await import('./ConfigStorage');
        const config = await configStorage.getConfig();
        if (config?.maxHistoryItems) {
          this.maxHistorySize = config.maxHistoryItems;
        }
      } catch (error) {
        console.warn('[HistoryStorage] Failed to load maxHistoryItems from config:', error);
      }

      await this.loadHistory();
      this.sortHistory();
      this.initialized = true;

      // 启动时清理孤儿数据
      this.cleanupOrphanedData().catch((error) => {
        console.error('[HistoryStorage] Failed to cleanup orphaned data on startup:', error);
      });
    } catch (error) {
      console.error('[HistoryStorage] Failed to initialize:', error);
      this.history = [];
      this.initialized = true;
    }
  }

  /**
   * 加载历史记录
   */
  private async loadHistory(): Promise<void> {
    const historyJson = await AsyncStorage.getItem(STORAGE_KEYS.HISTORY);
    const storedVersion = parseInt(
      (await AsyncStorage.getItem(STORAGE_KEYS.HISTORY_VERSION)) || '0',
      10
    );

    if (historyJson) {
      const parsedHistory: ClipboardItem[] = JSON.parse(historyJson);
      // 规范化所有记录，确保字段都有默认值
      this.history = parsedHistory.map(normalizeClipboardItem);

      // 执行版本迁移
      if (storedVersion < CURRENT_HISTORY_VERSION) {
        this.history = await this.runMigrations(this.history, storedVersion);
        console.log(
          `[HistoryStorage] Migrated history from version ${storedVersion} to ${CURRENT_HISTORY_VERSION}`
        );
        await this.saveHistory();
        await AsyncStorage.setItem(
          STORAGE_KEYS.HISTORY_VERSION,
          CURRENT_HISTORY_VERSION.toString()
        );
      }
    } else {
      this.history = [];
      // 新数据写入版本号
      if (CURRENT_HISTORY_VERSION > 0) {
        await AsyncStorage.setItem(
          STORAGE_KEYS.HISTORY_VERSION,
          CURRENT_HISTORY_VERSION.toString()
        );
      }
    }
  }

  /**
   * 执行数据迁移
   * @param items 历史记录数组
   * @param fromVersion 起始版本号
   * @returns 迁移后的记录数组
   */
  private async runMigrations(
    items: ClipboardItem[],
    fromVersion: number
  ): Promise<ClipboardItem[]> {
    let migratedItems = [...items];

    for (let v = fromVersion + 1; v <= CURRENT_HISTORY_VERSION; v++) {
      const migration = MIGRATIONS[v];
      if (migration) {
        console.log(`[HistoryStorage] Running migration to version ${v}`);
        migratedItems = migration(migratedItems);
      }
    }

    return migratedItems;
  }

  /**
   * 保存历史记录
   */
  private async saveHistory(): Promise<void> {
    try {
      await AsyncStorage.setItem(STORAGE_KEYS.HISTORY, JSON.stringify(this.history));
    } catch (error) {
      console.error('[HistoryStorage] Failed to save history:', error);
      throw error;
    }
  }

  /**
   * 添加历史记录
   */
  public async addItem(item: ClipboardItem): Promise<ClipboardItem> {
    if (!this.initialized) {
      await this.initialize();
    }

    // 处理文件复制逻辑
    let processedItem = { ...item };

    // 检查是否有文件数据且需要复制
    if (
      processedItem.hasData &&
      processedItem.fileUri &&
      processedItem.profileHash &&
      processedItem.dataName
    ) {
      try {
        // 获取历史记录目录
        const historyDir = getHistoryFileDir(processedItem.type, processedItem.profileHash);
        const historyDirUri = historyDir.uri;

        // 检查文件是否已经在历史记录目录中
        if (!processedItem.fileUri.startsWith(historyDirUri)) {
          // 读取源文件数据
          const sourceFile = new File(processedItem.fileUri);
          if (sourceFile.exists) {
            // 获取历史记录目录
            const historyDir = getHistoryFileDir(processedItem.type, processedItem.profileHash);

            // 确保历史记录目录存在
            if (!historyDir.exists) {
              historyDir.create();
            }

            // 创建目标文件
            const targetFile = new File(historyDir, processedItem.dataName);
            if (!targetFile.exists) {
              sourceFile.move(targetFile);
            }

            // 更新 fileUri 为新的路径
            processedItem.fileUri = targetFile.uri;
            console.log('[HistoryStorage] File moved to history directory:', targetFile.uri);
          }
        }
      } catch (error) {
        console.error('[HistoryStorage] Failed to move file to history directory:', error);
        // 继续执行，不阻止历史记录添加
      }
    }

    // 检查是否已存在相同 hash 的记录（不区分大小写）
    const existingIndex = this.history.findIndex(
      (h) => h.profileHash.toLowerCase() === processedItem.profileHash.toLowerCase()
    );

    let action: 'add' | 'update';
    let resultItem: ClipboardItem;

    if (existingIndex >= 0) {
      // 更新现有记录 - 参照桌面客户端 AddLocalProfile 逻辑
      const existing = this.history[existingIndex];

      // 如果现有记录没有 text 但新记录有，则更新 text
      const text = !existing.text && processedItem.text ? processedItem.text : existing.text;

      // 如果记录之前是软删除状态，需要恢复并触发同步
      const wasDeleted = existing.isDeleted === true;

      resultItem = {
        ...existing,
        text,
        fileUri: processedItem.fileUri ?? existing.fileUri,
        isLocalFileReady: true,
        isDeleted: false,
        lastModified: Date.now(),
        lastAccessed: Date.now(),
        version: existing.version + 1,
        syncStatus: wasDeleted ? HistorySyncStatus.LocalOnly : HistorySyncStatus.NeedSync,
      };

      // 从旧位置移除，重新插入到正确位置
      this.history.splice(existingIndex, 1);
      const updateIdx = this.findInsertIndex(resultItem);
      this.history.splice(updateIdx, 0, resultItem);
      action = 'update';
    } else {
      // 添加新记录，二分插入到正确位置
      resultItem = {
        ...processedItem,
        timestamp: processedItem.timestamp || Date.now(),
      };
      const insertIdx = this.findInsertIndex(resultItem);
      this.history.splice(insertIdx, 0, resultItem);
      action = 'add';

      // 清理超出数量的记录（仅清理 LocalOnly 状态的记录）
      await this.cleanupByCount(this.maxHistorySize);
    }

    await this.saveHistory();
    this.notifyChange(resultItem, action);
    return resultItem;
  }

  /**
   * 批量添加历史记录
   */
  public async addItems(items: ClipboardItem[]): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const addedItems: ClipboardItem[] = [];
    const updatedItems: ClipboardItem[] = [];

    for (const item of items) {
      const existingIndex = this.history.findIndex(
        (h) => h.profileHash.toLowerCase() === item.profileHash.toLowerCase()
      );

      if (existingIndex >= 0) {
        // 更新现有记录 - 参照桌面客户端 AddLocalProfile 逻辑
        const existing = this.history[existingIndex];
        const text = !existing.text && item.text ? item.text : existing.text;

        // 如果记录之前是软删除状态，需要恢复并触发同步
        const wasDeleted = existing.isDeleted === true;

        const updatedItem = {
          ...existing,
          text,
          fileUri: item.fileUri ?? existing.fileUri,
          isLocalFileReady: true,
          isDeleted: false,
          lastModified: Date.now(),
          lastAccessed: Date.now(),
          version: wasDeleted ? existing.version + 1 : existing.version,
          syncStatus: wasDeleted ? HistorySyncStatus.NeedSync : existing.syncStatus,
        };
        // 从旧位置移除，重新插入到正确位置
        this.history.splice(existingIndex, 1);
        const updateIdx = this.findInsertIndex(updatedItem);
        this.history.splice(updateIdx, 0, updatedItem);
        updatedItems.push(updatedItem);
      } else {
        const newItem = {
          ...item,
          timestamp: item.timestamp || Date.now(),
        };
        const insertIdx = this.findInsertIndex(newItem);
        this.history.splice(insertIdx, 0, newItem);
        addedItems.push(newItem);
      }
    }

    // 清理超出数量的记录（仅清理 LocalOnly 状态的记录）
    await this.cleanupByCount(this.maxHistorySize);

    await this.saveHistory();

    // 通知变更
    if (addedItems.length > 0) {
      this.notifyChangeBatch(addedItems, 'add');
    }
    if (updatedItems.length > 0) {
      this.notifyChangeBatch(updatedItems, 'update');
    }
  }

  /**
   * 根据 profileHash 获取历史记录
   */
  public async getItem(profileHash: string): Promise<ClipboardItem | null> {
    if (!this.initialized) {
      await this.initialize();
    }

    return (
      this.history.find((item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()) ||
      null
    );
  }

  /**
   * 根据 localClipboardHash 获取历史记录
   */
  public async getItemByLocalHash(localClipboardHash: string): Promise<ClipboardItem | null> {
    if (!this.initialized) {
      await this.initialize();
    }

    return this.history.find((item) => item.localClipboardHash === localClipboardHash) || null;
  }

  /**
   * 获取所有历史记录（排除软删除）
   */
  public async getAllItems(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    return this.history.filter((item) => !item.isDeleted);
  }

  /**
   * 获取所有历史记录（包括软删除）
   */
  public async getAllItemsIncludingDeleted(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    return [...this.history];
  }

  /**
   * 获取分页历史记录（排除软删除）
   */
  public async getItems(page: number = 1, pageSize: number = 20): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    const visibleItems = this.history.filter((item) => !item.isDeleted);
    const start = (page - 1) * pageSize;
    const end = start + pageSize;

    return visibleItems.slice(start, end);
  }

  /**
   * 搜索和过滤历史记录（排除软删除）
   * 注意：已移除分页参数，返回全部符合条件的记录，由虚拟列表处理性能
   */
  public async searchItems(
    filter?: HistoryFilter,
    sort?: HistorySort
  ): Promise<{ items: ClipboardItem[]; total: number }> {
    if (!this.initialized) {
      await this.initialize();
    }

    // 先过滤掉软删除的记录
    let filtered = this.history.filter((item) => !item.isDeleted);

    // 应用过滤器
    if (filter) {
      if (filter.type && filter.type.length > 0) {
        filtered = filtered.filter((item) => filter.type!.includes(item.type));
      }

      if (filter.startDate) {
        filtered = filtered.filter((item) => item.timestamp >= filter.startDate!);
      }

      if (filter.endDate) {
        filtered = filtered.filter((item) => item.timestamp <= filter.endDate!);
      }

      if (filter.keyword) {
        const keyword = filter.keyword.toLowerCase();
        filtered = filtered.filter(
          (item) =>
            item.text.toLowerCase().includes(keyword) ||
            (item.dataName && item.dataName.toLowerCase().includes(keyword))
        );
      }

      if (filter.starredOnly) {
        filtered = filtered.filter((item) => item.starred === true);
      }

      if (filter.syncedOnly) {
        filtered = filtered.filter((item) => item.synced === true);
      }

      if (filter.pinnedOnly) {
        filtered = filtered.filter((item) => item.pinned === true);
      }

      if (filter.localOnly) {
        filtered = filtered.filter((item) => item.isLocalFileReady === true);
      }

      if (filter.syncStatus && filter.syncStatus.length > 0) {
        filtered = filtered.filter(
          (item) => item.syncStatus !== undefined && filter.syncStatus!.includes(item.syncStatus)
        );
      }
    }

    // 应用排序（置顶记录始终在顶部）
    filtered.sort((a, b) => {
      // 置顶记录优先
      const aPinned = a.pinned ? 1 : 0;
      const bPinned = b.pinned ? 1 : 0;
      if (aPinned !== bPinned) {
        return bPinned - aPinned;
      }

      // 然后按指定字段排序
      if (sort) {
        let compareResult = 0;

        switch (sort.field) {
          case 'timestamp':
            compareResult = a.timestamp - b.timestamp;
            break;
          case 'useCount':
            compareResult = (a.useCount || 0) - (b.useCount || 0);
            break;
          case 'size':
            compareResult = (a.size || 0) - (b.size || 0);
            break;
          case 'lastAccessed':
            compareResult = (a.lastAccessed || a.timestamp) - (b.lastAccessed || b.timestamp);
            break;
        }

        return sort.order === 'desc' ? -compareResult : compareResult;
      }

      // 默认按时间倒序
      return b.timestamp - a.timestamp;
    });

    // 分页已移除，返回全部符合条件的记录（浅拷贝，避免外部持有内部数组的对象引用）
    return {
      items: filtered.map((item) => ({ ...item })),
      total: filtered.length,
    };
  }

  /**
   * 更新历史记录项
   */
  public async updateItem(profileHash: string, updates: Partial<ClipboardItem>): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const filteredUpdates = Object.fromEntries(
        Object.entries(updates).filter(([, value]) => value !== undefined)
      );
      this.history[index] = {
        ...this.history[index],
        ...filteredUpdates,
      };

      // 检查是否需要重新定位（排序字段变更时）
      if (this.shouldReposition(index)) {
        const item = this.history[index];
        this.history.splice(index, 1);
        const newIdx = this.findInsertIndex(item);
        this.history.splice(newIdx, 0, item);
      }

      await this.saveHistory();
      this.notifyChange(
        this.history.find((item) => item.profileHash.toLowerCase() === profileHash.toLowerCase())!,
        'update'
      );
    } else {
      throw new Error(`History item not found: ${profileHash}`);
    }
  }

  /**
   * 批量更新历史记录项
   */
  public async updateItems(
    updates: { profileHash: string; updates: Partial<ClipboardItem> }[]
  ): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const updatedItems: ClipboardItem[] = [];

    for (const { profileHash, updates: itemUpdates } of updates) {
      const index = this.history.findIndex(
        (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
      );

      if (index >= 0) {
        this.history[index] = {
          ...this.history[index],
          ...itemUpdates,
        };
        updatedItems.push(this.history[index]);
      }
    }

    if (updatedItems.length > 0) {
      await this.saveHistory();
      this.notifyChangeBatch(updatedItems, 'update');
    }
  }

  /**
   * 软删除历史记录项
   * 标记为已删除，保留记录用于同步，30天后物理删除
   */
  public async softDeleteItem(profileHash: string): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const item = this.history[index];

      // 更新为软删除状态
      this.history[index] = {
        ...item,
        isDeleted: true,
        lastModified: Date.now(),
        version: item.version + 1,
        syncStatus: HistorySyncStatus.NeedSync,
        isLocalFileReady: false,
      };

      await this.saveHistory();

      // 删除本地文件
      try {
        const { deleteHistoryFileDir } = await import('../utils/fileStorage');
        if (item.type && item.profileHash) {
          await deleteHistoryFileDir(item.type, item.profileHash);
          console.log(
            '[HistoryStorage] Soft deleted, file directory removed:',
            item.type,
            item.profileHash
          );
        }
      } catch (error) {
        console.error('[HistoryStorage] Failed to delete history file directory:', error);
      }

      this.notifyChange(this.history[index], 'update');
    }
  }

  /**
   * 批量软删除历史记录项
   */
  public async softDeleteItems(profileHashes: string[]): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const now = Date.now();
    const updatedItems: ClipboardItem[] = [];

    for (let i = 0; i < this.history.length; i++) {
      const item = this.history[i];
      if (profileHashes.some((hash) => hash.toLowerCase() === item.profileHash.toLowerCase())) {
        this.history[i] = {
          ...item,
          isDeleted: true,
          lastModified: now,
          version: (item.version || 0) + 1,
          syncStatus: HistorySyncStatus.NeedSync,
          isLocalFileReady: false,
        };
        updatedItems.push(this.history[i]);
      }
    }

    if (updatedItems.length > 0) {
      await this.saveHistory();

      // 批量删除本地文件
      try {
        const { deleteHistoryFileDir } = await import('../utils/fileStorage');
        for (const item of updatedItems) {
          if (item.type && item.profileHash) {
            try {
              await deleteHistoryFileDir(item.type, item.profileHash);
            } catch (error) {
              console.error(
                '[HistoryStorage] Failed to delete history file directory:',
                item.type,
                item.profileHash,
                error
              );
            }
          }
        }
      } catch (error) {
        console.error('[HistoryStorage] Failed to delete history file directories:', error);
      }

      this.notifyChangeBatch(updatedItems, 'update');
    }
  }

  /**
   * 物理删除历史记录项（用于孤儿数据清理和过期软删除清理）
   */
  public async physicalDeleteItem(profileHash: string): Promise<void> {
    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const item = this.history[index];
      this.history.splice(index, 1);
      await this.saveHistory();

      this.notifyChange(item, 'delete');

      try {
        const { deleteHistoryFileDir } = await import('../utils/fileStorage');
        if (item.type && item.profileHash) {
          await deleteHistoryFileDir(item.type, item.profileHash);
          console.log(
            '[HistoryStorage] History file directory deleted:',
            item.type,
            item.profileHash
          );
        }
      } catch (error) {
        console.error('[HistoryStorage] Failed to delete history file directory:', error);
      }
    }
  }

  /**
   * 批量物理删除历史记录项（一次性保存，减少IO）
   */
  public async physicalDeleteItems(profileHashes: string[]): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    const hashSet = new Set(profileHashes.map((h) => h.toLowerCase()));
    const deletedItems: ClipboardItem[] = [];

    this.history = this.history.filter((item) => {
      if (hashSet.has(item.profileHash.toLowerCase())) {
        deletedItems.push(item);
        return false;
      }
      return true;
    });

    if (deletedItems.length > 0) {
      await this.saveHistory();
      this.notifyChangeBatch(deletedItems, 'delete');

      for (const item of deletedItems) {
        try {
          const { deleteHistoryFileDir } = await import('../utils/fileStorage');
          if (item.type && item.profileHash) {
            await deleteHistoryFileDir(item.type, item.profileHash);
          }
        } catch (error) {
          console.error('[HistoryStorage] Failed to delete history file directory:', error);
        }
      }
    }

    return deletedItems;
  }

  /**
   * 清理过期的软删除记录（30天后物理删除）
   */
  public async cleanupExpiredSoftDeletes(): Promise<number> {
    if (!this.initialized) {
      await this.initialize();
    }

    const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;
    const cutoffTime = Date.now() - THIRTY_DAYS_MS;

    const expiredItems = this.history.filter(
      (item) => item.isDeleted && item.lastModified && item.lastModified < cutoffTime
    );

    if (expiredItems.length === 0) {
      return 0;
    }

    console.log(`[HistoryStorage] Cleaning up ${expiredItems.length} expired soft-deleted records`);

    for (const item of expiredItems) {
      await this.physicalDeleteItem(item.profileHash);
    }

    return expiredItems.length;
  }

  /**
   * 获取所有软删除的记录
   */
  public async getSoftDeletedItems(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    return this.history.filter((item) => item.isDeleted);
  }

  /**
   * 恢复软删除的记录
   */
  public async restoreSoftDeletedItem(profileHash: string): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0 && this.history[index].isDeleted) {
      this.history[index] = {
        ...this.history[index],
        isDeleted: false,
        lastModified: Date.now(),
        version: this.history[index].version + 1,
        syncStatus: HistorySyncStatus.NeedSync,
      };

      await this.saveHistory();
      this.notifyChange(this.history[index], 'update');
    }
  }

  /**
   * 标记/取消标记历史记录
   */
  public async toggleStar(profileHash: string): Promise<boolean> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const item = this.history[index];
      item.starred = !item.starred;
      item.lastModified = Date.now();
      item.version = item.version + 1;
      if (item.syncStatus !== HistorySyncStatus.LocalOnly) {
        item.syncStatus = HistorySyncStatus.NeedSync;
      }
      await this.saveHistory();
      this.notifyChange(item, 'update');
      return item.starred;
    }

    return false;
  }

  /**
   * 置顶/取消置顶历史记录
   */
  public async togglePin(profileHash: string): Promise<boolean> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const item = this.history[index];
      item.pinned = !item.pinned;
      item.lastModified = Date.now();
      item.version = item.version + 1;
      if (item.syncStatus !== HistorySyncStatus.LocalOnly) {
        item.syncStatus = HistorySyncStatus.NeedSync;
      }
      await this.saveHistory();
      this.notifyChange(item, 'update');
      return item.pinned;
    }

    return false;
  }

  /**
   * 更新最后访问时间（复制记录时调用）
   * 同时检查是否需要重新定位，并触发变更通知
   */
  public async updateLastAccessed(profileHash: string): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      this.history[index].lastAccessed = Date.now();

      // 检查是否需要重新定位（当按 lastAccessed 排序时）
      if (this.shouldReposition(index)) {
        const item = this.history[index];
        this.history.splice(index, 1);
        const newIdx = this.findInsertIndex(item);
        this.history.splice(newIdx, 0, item);
      }

      await this.saveHistory();
      this.notifyChange(
        this.history.find((item) => item.profileHash.toLowerCase() === profileHash.toLowerCase())!,
        'update'
      );
    }
  }

  /**
   * 更新同步状态
   */
  public async updateSyncStatus(
    profileHash: string,
    syncStatus: number,
    version?: number
  ): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      this.history[index].syncStatus = syncStatus;
      if (version !== undefined) {
        this.history[index].version = version;
      }
      await this.saveHistory();
      this.notifyChange(this.history[index], 'update');
    }
  }

  /**
   * 获取需要同步的记录（syncStatus === NeedSync）
   */
  public async getNeedSyncItems(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    const { HistorySyncStatus } = await import('../types/clipboard');
    return this.history.filter((item) => item.syncStatus === HistorySyncStatus.NeedSync);
  }

  /**
   * 获取本地记录（syncStatus === LocalOnly 或 undefined）
   */
  public async getLocalOnlyItems(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    const { HistorySyncStatus } = await import('../types/clipboard');
    return this.history.filter(
      (item) => item.syncStatus === HistorySyncStatus.LocalOnly || item.syncStatus === undefined
    );
  }

  /**
   * 获取服务器记录（isLocalFileReady === false 且 syncStatus === Synced）
   */
  public async getServerOnlyItems(): Promise<ClipboardItem[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    const { HistorySyncStatus } = await import('../types/clipboard');
    return this.history.filter(
      (item) => item.syncStatus === HistorySyncStatus.Synced && item.isLocalFileReady === false
    );
  }

  /**
   * 增加使用次数
   */
  public async incrementUseCount(profileHash: string): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const index = this.history.findIndex(
      (item) => item.profileHash.toLowerCase() === profileHash.toLowerCase()
    );

    if (index >= 0) {
      const item = this.history[index];
      item.useCount = (item.useCount || 0) + 1;
      await this.saveHistory();
    }
  }

  /**
   * 获取历史记录数量（排除软删除）
   */
  public async getCount(): Promise<number> {
    if (!this.initialized) {
      await this.initialize();
    }

    return this.history.filter((item) => !item.isDeleted).length;
  }

  /**
   * 获取历史记录统计信息
   */
  public async getStats(): Promise<{
    total: number;
    byType: Record<string, number>;
    totalSize: number;
    starred: number;
    synced: number;
    pinned: number;
    localOnly: number;
    serverOnly: number;
  }> {
    if (!this.initialized) {
      await this.initialize();
    }

    const { HistorySyncStatus } = await import('../types/clipboard');

    const stats = {
      total: this.history.length,
      byType: {} as Record<string, number>,
      totalSize: 0,
      starred: 0,
      synced: 0,
      pinned: 0,
      localOnly: 0,
      serverOnly: 0,
    };

    this.history.forEach((item) => {
      // 按类型统计
      stats.byType[item.type] = (stats.byType[item.type] || 0) + 1;

      // 总大小
      if (item.size) {
        stats.totalSize += item.size;
      }

      // 标记数
      if (item.starred) {
        stats.starred++;
      }

      // 已同步数
      if (item.synced) {
        stats.synced++;
      }

      // 置顶数
      if (item.pinned) {
        stats.pinned++;
      }

      // 本地记录数
      if (item.syncStatus === HistorySyncStatus.LocalOnly || item.syncStatus === undefined) {
        stats.localOnly++;
      }

      // 仅服务器记录数
      if (item.syncStatus === HistorySyncStatus.Synced && item.isLocalFileReady === false) {
        stats.serverOnly++;
      }
    });

    return stats;
  }

  /**
   * 清空历史记录
   */
  public async clear(): Promise<void> {
    // 清空内存中的历史记录
    this.history = [];
    // 从AsyncStorage中移除历史记录
    await AsyncStorage.removeItem(STORAGE_KEYS.HISTORY);

    // 删除历史记录文件夹下的所有文件
    try {
      const { initFileStorage } = await import('../utils/fileStorage');
      await initFileStorage();

      const { HISTORY_BASE_DIR } = await import('../utils/fileStorage');
      if (HISTORY_BASE_DIR.exists) {
        const entries = HISTORY_BASE_DIR.list();
        for (const entry of entries) {
          try {
            entry.delete();
          } catch (error) {
            console.error('[HistoryStorage] Failed to delete history entry:', error);
          }
        }
        console.log('[HistoryStorage] History files cleared');
      }
    } catch (error) {
      console.error('[HistoryStorage] Failed to clear history files:', error);
    }
  }

  /**
   * 清空旧记录（保留最近的 N 条）
   */
  public async cleanOldItems(keepCount: number = 100): Promise<number> {
    if (!this.initialized) {
      await this.initialize();
    }

    const originalCount = this.history.length;

    if (originalCount > keepCount) {
      this.history = this.history.slice(0, keepCount);
      await this.saveHistory();
      return originalCount - keepCount;
    }

    return 0;
  }

  /**
   * 设置最大历史记录大小
   */
  public setMaxHistorySize(size: number): void {
    if (size < 10) {
      throw new Error('Max history size must be at least 10');
    }
    this.maxHistorySize = size;
  }

  /**
   * 清理超出数量的记录（仅清理 LocalOnly 状态的记录）
   * @param maxCount 最大保留数量，0 表示不限制
   * @returns 删除的记录数量
   */
  public async cleanupByCount(maxCount: number = this.maxHistorySize): Promise<number> {
    if (!this.initialized) {
      await this.initialize();
    }

    console.log(
      `[HistoryStorage] cleanupByCount called: maxCount=${maxCount}, current maxHistorySize=${this.maxHistorySize}`
    );

    if (maxCount === 0) {
      console.log('[HistoryStorage] cleanupByCount skipped: maxCount is 0');
      return 0;
    }

    const { HistorySyncStatus } = await import('../types/clipboard');

    const localOnlyItems = this.history.filter(
      (item) =>
        (item.syncStatus === HistorySyncStatus.LocalOnly || item.syncStatus === undefined) &&
        !item.starred &&
        !item.pinned
    );

    console.log(
      `[HistoryStorage] cleanupByCount: total items=${this.history.length}, localOnly items=${localOnlyItems.length}, maxCount=${maxCount}`
    );

    if (localOnlyItems.length <= maxCount) {
      console.log('[HistoryStorage] cleanupByCount skipped: no items to delete');
      return 0;
    }

    localOnlyItems.sort((a, b) => a.timestamp - b.timestamp);

    const toDeleteCount = localOnlyItems.length - maxCount;
    const toDeleteHashes = new Set(
      localOnlyItems.slice(0, toDeleteCount).map((item) => item.profileHash.toLowerCase())
    );

    const itemsToDelete = this.history.filter((item) =>
      toDeleteHashes.has(item.profileHash.toLowerCase())
    );

    this.history = this.history.filter(
      (item) => !toDeleteHashes.has(item.profileHash.toLowerCase())
    );

    await this.saveHistory();

    for (const item of itemsToDelete) {
      try {
        const { deleteHistoryFileDir } = await import('../utils/fileStorage');
        if (item.type && item.profileHash) {
          await deleteHistoryFileDir(item.type, item.profileHash);
        }
      } catch (error) {
        console.error('[HistoryStorage] Failed to delete history file directory:', error);
      }
    }

    if (itemsToDelete.length > 0) {
      this.notifyChangeBatch(itemsToDelete, 'delete');
    }

    console.log(`[HistoryStorage] Cleaned up ${toDeleteCount} LocalOnly records`);
    return toDeleteCount;
  }

  /**
   * 清理孤儿数据（文件存在但记录不存在的数据）
   */
  public async cleanupOrphanedData(): Promise<number> {
    if (!this.initialized) {
      await this.initialize();
    }

    let cleanedCount = 0;

    try {
      const { initFileStorage, HISTORY_BASE_DIR } = await import('../utils/fileStorage');
      await initFileStorage();

      if (!HISTORY_BASE_DIR.exists) {
        return 0;
      }

      const validProfileHashes = new Set(
        this.history.map((item) => item.profileHash.toLowerCase())
      );

      const typeDirs = HISTORY_BASE_DIR.list();
      for (const typeDir of typeDirs) {
        if (!('isDirectory' in typeDir) || !typeDir.isDirectory) continue;

        const hashDirs = (typeDir as Directory).list();
        for (const hashDir of hashDirs) {
          if (!('isDirectory' in hashDir) || !hashDir.isDirectory) continue;

          const hashFromDir = (hashDir as Directory).name?.toLowerCase();
          if (hashFromDir && !validProfileHashes.has(hashFromDir)) {
            try {
              (hashDir as Directory).delete();
              cleanedCount++;
              console.log(`[HistoryStorage] Cleaned orphaned directory: ${hashDir.uri}`);
            } catch (error) {
              console.error('[HistoryStorage] Failed to delete orphaned directory:', error);
            }
          }
        }
      }

      if (cleanedCount > 0) {
        console.log(`[HistoryStorage] Cleaned ${cleanedCount} orphaned data directories`);
      }
    } catch (error) {
      console.error('[HistoryStorage] Failed to cleanup orphaned data:', error);
    }

    return cleanedCount;
  }
}

// 导出单例
export const historyStorage = HistoryStorage.getInstance();
