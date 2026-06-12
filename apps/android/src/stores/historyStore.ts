/**
 * History Store
 * 历史记录状态管理 - 使用 Zustand
 */

import { create } from 'zustand';
import { ClipboardItem } from '../types/clipboard';
import { HistoryFilter, HistorySort } from '../types/storage';
import { historyStorage } from '../services';

/**
 * 历史记录状态接口
 */
interface HistoryState {
  // 状态
  /** 历史记录列表 */
  items: ClipboardItem[];

  /** 总记录数 */
  totalCount: number;

  /** 当前过滤器 */
  filter: HistoryFilter | null;

  /** 当前排序 */
  sort: HistorySort | null;

  /** 是否正在加载 */
  isLoading: boolean;

  /** 错误信息 */
  error: string | null;

  /** 选中的项目ID */
  selectedIds: Set<string>;

  /** 最后添加项目的时间戳 */
  lastAddedTimestamp: number;

  /** 最后删除的 profileHash 列表（用于通知其他组件） */
  lastDeletedHashes: string[];

  /** 是否清空了所有历史记录 */
  historyCleared: boolean;

  // 动作
  /** 加载历史记录 */
  loadItems: () => Promise<void>;

  /** 搜索历史记录 */
  searchItems: (filter?: HistoryFilter, sort?: HistorySort) => Promise<void>;

  /** 添加历史记录 */
  addItem: (item: ClipboardItem) => Promise<ClipboardItem>;

  /** 批量添加历史记录 */
  addItems: (items: ClipboardItem[]) => Promise<void>;

  /** 更新历史记录 */
  updateItem: (id: string, updates: Partial<ClipboardItem>) => Promise<void>;

  /** 删除历史记录 */
  deleteItem: (id: string) => Promise<void>;

  /** 批量删除 */
  deleteItems: (ids: string[]) => Promise<void>;

  /** 切换标记 */
  toggleStar: (id: string) => Promise<void>;

  /** 切换置顶 */
  togglePin: (id: string) => Promise<void>;

  /** 增加使用次数 */
  incrementUseCount: (id: string) => Promise<void>;

  /** 清空历史记录 */
  clearHistory: () => Promise<void>;

  /** 设置过滤器 */
  setFilter: (filter: HistoryFilter | null) => void;

  /** 设置排序 */
  setSort: (sort: HistorySort | null) => void;

  /** 切换选中 */
  toggleSelection: (id: string) => void;

  /** 全选 */
  selectAll: () => void;

  /** 取消全选 */
  clearSelection: () => void;

  /** 删除选中项 */
  deleteSelected: () => Promise<void>;

  /** 清除错误 */
  clearError: () => void;

  /** 清除删除状态（消费 lastDeletedHashes 和 historyCleared） */
  clearDeletedState: () => void;

  /** 刷新 */
  refresh: () => Promise<void>;

  /** 处理存储变更（实时更新） */
  handleStorageChange: (items: ClipboardItem[], action: 'add' | 'update' | 'delete') => void;

  /** 重置 */
  reset: () => void;
}

/**
 * 初始状态
 */
const initialState = {
  items: [],
  totalCount: 0,
  filter: null,
  sort: null,
  isLoading: false,
  error: null,
  selectedIds: new Set<string>(),
  lastAddedTimestamp: 0,
  lastDeletedHashes: [],
  historyCleared: false,
};

/**
 * 创建历史记录 Store
 */
export const useHistoryStore = create<HistoryState>((set, get) => ({
  ...initialState,

  loadItems: async () => {
    set({ isLoading: true, error: null });

    try {
      const { filter, sort } = get();

      const effectiveSort: HistorySort = sort || { field: 'timestamp', order: 'desc' };

      const result = await historyStorage.searchItems(filter || undefined, effectiveSort);
      set({
        items: result.items,
        totalCount: result.total,
        isLoading: false,
      });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to load history';
      set({ error: errorMessage, isLoading: false });
    }
  },

  searchItems: async (filter, sort) => {
    // 不传 sort 时保留当前 sort 配置，避免覆盖 loadSortSetting 设置的值
    const effectiveSort = sort !== undefined ? sort : get().sort;
    set({ isLoading: true, error: null, filter, sort: effectiveSort });

    try {
      const result = await historyStorage.searchItems(filter, effectiveSort || undefined);

      set({
        items: result.items,
        totalCount: result.total,
        isLoading: false,
      });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to search history';
      set({ error: errorMessage, isLoading: false });
    }
  },

  addItem: async (item: ClipboardItem) => {
    set({ error: null });

    try {
      const savedItem = await historyStorage.addItem(item);

      // 更新最后添加时间戳
      set({ lastAddedTimestamp: Date.now() });

      // 刷新当前页
      await get().refresh();

      return savedItem;
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to add item';
      set({ error: errorMessage });
      return item;
    }
  },

  addItems: async (items: ClipboardItem[]) => {
    set({ error: null });

    try {
      await historyStorage.addItems(items);

      set({ lastAddedTimestamp: Date.now() });

      await get().refresh();
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to add items';
      set({ error: errorMessage });
    }
  },

  updateItem: async (profileHash: string, updates: Partial<ClipboardItem>) => {
    set({ error: null });

    try {
      await historyStorage.updateItem(profileHash, updates);

      // 更新本地状态
      set((state) => ({
        items: state.items.map((item) =>
          item.profileHash === profileHash ? { ...item, ...updates } : item
        ),
      }));
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to update item';
      set({ error: errorMessage });
    }
  },

  deleteItem: async (profileHash: string) => {
    set({ error: null });

    try {
      await historyStorage.softDeleteItem(profileHash);
      set({ lastDeletedHashes: [profileHash] });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to delete item';
      set({ error: errorMessage });
    }
  },

  deleteItems: async (profileHashes: string[]) => {
    set({ error: null });

    try {
      await historyStorage.softDeleteItems(profileHashes);
      set({ lastDeletedHashes: profileHashes });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to delete items';
      set({ error: errorMessage });
    }
  },

  toggleStar: async (profileHash: string) => {
    set({ error: null });

    try {
      await historyStorage.toggleStar(profileHash);
      // 状态更新由 handleStorageChange 处理
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to toggle star';
      set({ error: errorMessage });
    }
  },

  togglePin: async (profileHash: string) => {
    set({ error: null });

    try {
      await historyStorage.togglePin(profileHash);
      // 状态更新由 handleStorageChange 处理
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to toggle pin';
      set({ error: errorMessage });
    }
  },

  incrementUseCount: async (profileHash: string) => {
    try {
      await historyStorage.incrementUseCount(profileHash);

      // 更新本地状态（可选）
      set((state) => ({
        items: state.items.map((item) => {
          if (item.profileHash === profileHash) {
            const useCount = (item.useCount || 0) + 1;
            return { ...item, useCount };
          }
          return item;
        }),
      }));
    } catch (error) {
      // 静默失败，不影响用户体验
      console.error('Failed to increment use count:', error);
    }
  },

  clearHistory: async () => {
    set({ isLoading: true, error: null });

    try {
      await historyStorage.clear();
      set({
        items: [],
        totalCount: 0,
        selectedIds: new Set(),
        isLoading: false,
        historyCleared: true,
      });
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to clear history';
      set({ error: errorMessage, isLoading: false });
    }
  },

  setFilter: (filter: HistoryFilter | null) => {
    set({ filter });
  },

  setSort: (sort: HistorySort | null) => {
    set({ sort });
    if (sort) {
      historyStorage.setSortConfig(sort);
    }
  },

  toggleSelection: (id: string) => {
    set((state) => {
      const newSelectedIds = new Set(state.selectedIds);
      if (newSelectedIds.has(id)) {
        newSelectedIds.delete(id);
      } else {
        newSelectedIds.add(id);
      }
      return { selectedIds: newSelectedIds };
    });
  },

  selectAll: () => {
    set((state) => ({
      selectedIds: new Set(state.items.map((item) => item.profileHash)),
    }));
  },

  clearSelection: () => {
    set({ selectedIds: new Set() });
  },

  deleteSelected: async () => {
    const { selectedIds } = get();
    if (selectedIds.size > 0) {
      await get().deleteItems([...selectedIds]);
    }
  },

  clearError: () => {
    set({ error: null });
  },

  clearDeletedState: () => {
    set({ lastDeletedHashes: [], historyCleared: false });
  },

  refresh: async () => {
    await get().loadItems();
  },

  handleStorageChange: (changedItems: ClipboardItem[], action: 'add' | 'update' | 'delete') => {
    const { items, filter } = get();

    // 检查是否匹配当前筛选条件
    const matchesFilter = (record: ClipboardItem): boolean => {
      if (!filter) return true;
      if (filter.keyword && !record.text.toLowerCase().includes(filter.keyword.toLowerCase())) {
        return false;
      }
      if (filter.type && filter.type.length > 0 && !filter.type.includes(record.type)) {
        return false;
      }
      if (filter.starredOnly && !record.starred) {
        return false;
      }
      if (filter.pinnedOnly && !record.pinned) {
        return false;
      }
      if (filter.syncedOnly && record.syncStatus !== 1) {
        return false;
      }
      if (filter.localOnly && !record.hasData) {
        return false;
      }
      if (
        filter.syncStatus &&
        filter.syncStatus.length > 0 &&
        !filter.syncStatus.includes(record.syncStatus ?? 0)
      ) {
        return false;
      }
      if (filter.transferringOnly && record.syncStatus !== 2) {
        return false;
      }
      return true;
    };

    if (action === 'add') {
      // 新增记录：批量插入到列表开头
      // 过滤掉已存在的记录（避免 refresh 和 notifyChange 竞争导致重复插入）
      const existingHashes = new Set(items.map((i) => i.profileHash.toLowerCase()));
      const filteredItems = changedItems.filter(
        (item) =>
          !item.isDeleted &&
          matchesFilter(item) &&
          !existingHashes.has(item.profileHash.toLowerCase())
      );
      if (filteredItems.length > 0) {
        set({
          items: [...filteredItems, ...items],
          totalCount: get().totalCount + filteredItems.length,
          lastAddedTimestamp: Date.now(),
        });
      }
    } else if (action === 'update') {
      // 获取排序配置
      const sortField = get().sort?.field || 'timestamp';
      const sortOrder = get().sort?.order || 'desc';
      const isDesc = sortOrder === 'desc';

      /**
       * 获取排序字段的值
       */
      const getSortValue = (item: ClipboardItem): number => {
        switch (sortField) {
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
      };

      /**
       * 二分查找插入位置（参照桌面端 InsertHistoryInOrder）
       */
      const findInsertIndex = (arr: ClipboardItem[], item: ClipboardItem): number => {
        // 先确定 pinned 区域的边界
        const isPinned = item.pinned;
        let searchStart = 0;
        let searchEnd = arr.length;

        if (isPinned) {
          // pinned 只在 pinned 区域内查找
          searchEnd = arr.findIndex((i) => !i.pinned);
          if (searchEnd === -1) searchEnd = arr.length;
        } else {
          // 非 pinned 从第一个非 pinned 开始
          searchStart = arr.findIndex((i) => !i.pinned);
          if (searchStart === -1) searchStart = arr.length;
        }

        const targetVal = getSortValue(item);
        let low = searchStart;
        let high = searchEnd;

        while (low < high) {
          const mid = (low + high) >> 1;
          const midVal = getSortValue(arr[mid]);
          const shouldGoLeft = isDesc ? midVal <= targetVal : midVal >= targetVal;
          if (shouldGoLeft) {
            high = mid;
          } else {
            low = mid + 1;
          }
        }

        return low;
      };

      // 检查是否有软删除的项
      const softDeletedHashes = new Set(
        changedItems.filter((item) => item.isDeleted).map((item) => item.profileHash.toLowerCase())
      );
      const updatedItems = changedItems.filter((item) => !item.isDeleted);

      // 创建新数组（移除软删除的项）
      const newItems = items.filter(
        (item) => !softDeletedHashes.has(item.profileHash.toLowerCase())
      );
      let addedCount = 0;

      for (const changedItem of updatedItems) {
        const existingIndex = newItems.findIndex(
          (i) => i.profileHash.toLowerCase() === changedItem.profileHash.toLowerCase()
        );

        if (existingIndex >= 0) {
          const oldItem = newItems[existingIndex];
          const filteredUpdates = Object.fromEntries(
            Object.entries(changedItem).filter(([, value]) => value !== undefined)
          );
          const updatedItem = { ...oldItem, ...filteredUpdates };

          // 检查是否需要重新定位
          const sortFieldChanged =
            (sortField === 'lastAccessed' && changedItem.lastAccessed !== oldItem.lastAccessed) ||
            (sortField === 'timestamp' && changedItem.timestamp !== oldItem.timestamp) ||
            (sortField === 'useCount' && changedItem.useCount !== oldItem.useCount) ||
            (sortField === 'size' && changedItem.size !== oldItem.size);
          const pinnedChanged = changedItem.pinned !== oldItem.pinned;

          if (sortFieldChanged || pinnedChanged) {
            // 删除后重新按序插入（参照桌面端 RemoveInsert）
            newItems.splice(existingIndex, 1);
            const insertIdx = findInsertIndex(newItems, updatedItem);
            newItems.splice(insertIdx, 0, updatedItem);
          } else {
            // 只更新数据，不移动位置
            newItems[existingIndex] = updatedItem;
          }
        } else if (matchesFilter(changedItem)) {
          // 新项按序插入
          const insertIdx = findInsertIndex(newItems, changedItem);
          newItems.splice(insertIdx, 0, changedItem);
          addedCount++;
        }
      }

      const removedCount = softDeletedHashes.size;
      if (removedCount > 0 || addedCount > 0) {
        set({
          items: [...newItems],
          totalCount: Math.max(0, get().totalCount - removedCount + addedCount),
          selectedIds: new Set(
            [...get().selectedIds].filter((id) => !softDeletedHashes.has(id.toLowerCase()))
          ),
          ...(addedCount > 0 ? { lastAddedTimestamp: Date.now() } : {}),
        });
      } else {
        // 即使只有位置变化也需要创建新数组引用，否则 React 不会重渲染
        set({ items: [...newItems] });
      }
    } else if (action === 'delete') {
      // 删除记录：批量从列表中移除
      const deletedHashes = new Set(changedItems.map((i) => i.profileHash.toLowerCase()));
      set({
        items: items.filter((i) => !deletedHashes.has(i.profileHash.toLowerCase())),
        totalCount: Math.max(0, get().totalCount - changedItems.length),
      });
    }
  },

  reset: () => {
    set(initialState);
  },
}));
