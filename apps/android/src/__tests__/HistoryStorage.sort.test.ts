/**
 * HistoryStorage 排序测试
 * 验证历史记录插入、更新后排序行为符合预期
 */

import { HistoryStorage } from '../services/HistoryStorage';
import { ClipboardItem, HistorySyncStatus } from '../types/clipboard';

jest.mock('@react-native-async-storage/async-storage', () => ({
  getItem: jest.fn().mockResolvedValue(null),
  setItem: jest.fn().mockResolvedValue(undefined),
  removeItem: jest.fn().mockResolvedValue(undefined),
}));

jest.mock('expo-file-system', () => ({
  File: jest.fn().mockImplementation((pathOrDir: unknown, name?: string) => ({
    exists: false,
    uri: name ? `file://test/${name}` : 'file://test',
    move: jest.fn(),
  })),
  Directory: jest.fn().mockImplementation(() => ({
    exists: true,
    create: jest.fn(),
    uri: 'file://test/history',
  })),
}));

jest.mock('../utils/fileStorage', () => ({
  getHistoryFileDir: jest.fn().mockReturnValue({
    uri: 'file://test/history',
    exists: true,
    create: jest.fn(),
  }),
}));

jest.mock('../services/ConfigStorage', () => ({
  configStorage: {
    getConfig: jest.fn().mockResolvedValue({ maxHistoryItems: 1000 }),
  },
}));

// cleanupByCount 中有 dynamic import，需要 mock 整个类型模块
jest.mock('../types/clipboard', () => {
  const actual = jest.requireActual('../types/clipboard');
  return actual;
});

/**
 * 创建测试用 ClipboardItem
 */
function createItem(
  profileHash: string,
  timestamp: number,
  overrides?: Partial<ClipboardItem>
): ClipboardItem {
  return {
    type: 'Text',
    text: `item-${profileHash}`,
    profileHash,
    hasData: false,
    size: 0,
    timestamp,
    starred: false,
    syncStatus: HistorySyncStatus.LocalOnly,
    version: 0,
    lastModified: timestamp,
    lastAccessed: timestamp,
    isDeleted: false,
    pinned: false,
    isLocalFileReady: true,
    ...overrides,
  };
}

/**
 * 提取 profileHash 列表，方便断言比对顺序
 */
function hashes(items: ClipboardItem[]): string[] {
  return items.map((i) => i.profileHash);
}

describe('HistoryStorage 排序', () => {
  let storage: HistoryStorage;

  beforeEach(async () => {
    jest.clearAllMocks();
    // 重置单例
    (HistoryStorage as unknown as { instance: null }).instance = null;
    storage = HistoryStorage.getInstance();
    await storage.initialize();

    // cleanupByCount 内有 dynamic import，在测试环境中不可用，直接 mock 掉
    jest.spyOn(storage as never, 'cleanupByCount' as never).mockResolvedValue(0 as never);
  });

  // =========================================================
  // 1. 去分页：searchItems 应返回所有命中项
  // =========================================================
  describe('去分页', () => {
    it('searchItems 应返回所有项，不受默认分页限制', async () => {
      for (let i = 0; i < 25; i++) {
        await storage.addItem(createItem(`h${i}`, 1000 + i));
      }

      const result = await storage.searchItems(undefined, {
        field: 'timestamp',
        order: 'desc',
      });

      expect(result.items).toHaveLength(25);
      expect(result.total).toBe(25);
    });
  });

  // =========================================================
  // 2. 插入后排序
  //    按时间倒序插入三条乱序记录，getAllItems 应按 timestamp desc
  // =========================================================
  describe('插入后保持排序', () => {
    it('乱序 addItem 后 getAllItems 仍按 timestamp desc', async () => {
      await storage.addItem(createItem('a', 100));
      await storage.addItem(createItem('b', 300));
      await storage.addItem(createItem('c', 200));

      const items = await storage.getAllItems();
      expect(hashes(items)).toEqual(['b', 'c', 'a']);
    });

    it('同一 timestamp 插入时保持稳定顺序（后插在前）', async () => {
      await storage.addItem(createItem('x', 500));
      await storage.addItem(createItem('y', 500));

      const items = await storage.getAllItems();
      // 后插入的 y 应排在同 ts 的 x 前面（较新）
      expect(hashes(items)).toEqual(['y', 'x']);
    });
  });

  // =========================================================
  // 3. 重复插入（addItem 同 hash）更新 lastAccessed 并重新定位
  // =========================================================
  describe('重复插入更新位置', () => {
    it('re-add 已有 hash 后，lastAccessed 更新，按 lastAccessed desc 排在首位', async () => {
      await storage.addItem(createItem('a', 100, { lastAccessed: 100 }));
      await storage.addItem(createItem('b', 200, { lastAccessed: 200 }));
      await storage.addItem(createItem('c', 300, { lastAccessed: 300 }));

      // re-add a → lastAccessed 更新为 now
      await storage.addItem(createItem('a', 100));

      const result = await storage.searchItems(undefined, {
        field: 'lastAccessed',
        order: 'desc',
      });

      // a 的 lastAccessed 被更新为 Date.now()，应排第一
      expect(result.items[0].profileHash).toBe('a');
    });
  });

  // =========================================================
  // 4. updateItem 改变排序字段后位置应重排
  // =========================================================
  describe('updateItem 后重排', () => {
    it('更新 lastAccessed 后，按 lastAccessed desc 排序正确', async () => {
      await storage.addItem(createItem('a', 100, { lastAccessed: 100 }));
      await storage.addItem(createItem('b', 200, { lastAccessed: 200 }));
      await storage.addItem(createItem('c', 300, { lastAccessed: 300 }));

      // 把 a 的 lastAccessed 设为最大
      await storage.updateItem('a', { lastAccessed: 999 });

      const result = await storage.searchItems(undefined, {
        field: 'lastAccessed',
        order: 'desc',
      });
      expect(hashes(result.items)).toEqual(['a', 'c', 'b']);
    });

    it('更新 timestamp 后，按 timestamp desc 排序正确', async () => {
      await storage.addItem(createItem('a', 100));
      await storage.addItem(createItem('b', 200));
      await storage.addItem(createItem('c', 300));

      await storage.updateItem('a', { timestamp: 999 });

      const result = await storage.searchItems(undefined, {
        field: 'timestamp',
        order: 'desc',
      });
      expect(hashes(result.items)).toEqual(['a', 'c', 'b']);
    });
  });

  // =========================================================
  // 5. 置顶记录：searchItems 中 pinned 应始终在前
  // =========================================================
  describe('置顶排序', () => {
    it('pinned 项排在 searchItems 结果最前', async () => {
      await storage.addItem(createItem('a', 100));
      await storage.addItem(createItem('b', 200, { pinned: true }));
      await storage.addItem(createItem('c', 300));

      const result = await storage.searchItems(undefined, {
        field: 'timestamp',
        order: 'desc',
      });

      expect(result.items[0].profileHash).toBe('b');
      expect(hashes(result.items)).toEqual(['b', 'c', 'a']);
    });

    it('togglePin 后项移到 searchItems 首位', async () => {
      await storage.addItem(createItem('a', 100));
      await storage.addItem(createItem('b', 200));
      await storage.addItem(createItem('c', 300));

      await storage.togglePin('a');

      const result = await storage.searchItems(undefined, {
        field: 'timestamp',
        order: 'desc',
      });

      expect(result.items[0].profileHash).toBe('a');
    });
  });

  // =========================================================
  // 6. setSortConfig 切换排序模式后，内部数组重排
  // =========================================================
  describe('setSortConfig 切换排序', () => {
    it('从 timestamp desc 切换到 lastAccessed desc，getAllItems 顺序改变', async () => {
      // a: ts=100, la=300; b: ts=200, la=100; c: ts=300, la=200
      await storage.addItem(createItem('a', 100, { lastAccessed: 300 }));
      await storage.addItem(createItem('b', 200, { lastAccessed: 100 }));
      await storage.addItem(createItem('c', 300, { lastAccessed: 200 }));

      // 默认 timestamp desc → [c, b, a]
      let items = await storage.getAllItems();
      expect(hashes(items)).toEqual(['c', 'b', 'a']);

      // 切换到 lastAccessed desc → [a, c, b]
      storage.setSortConfig({ field: 'lastAccessed', order: 'desc' });
      items = await storage.getAllItems();
      expect(hashes(items)).toEqual(['a', 'c', 'b']);
    });

    it('切换排序后 addItem 仍按新排序规则插入', async () => {
      storage.setSortConfig({ field: 'lastAccessed', order: 'desc' });

      await storage.addItem(createItem('a', 100, { lastAccessed: 100 }));
      await storage.addItem(createItem('b', 200, { lastAccessed: 300 }));
      await storage.addItem(createItem('c', 300, { lastAccessed: 200 }));

      const items = await storage.getAllItems();
      // lastAccessed desc: b(300), c(200), a(100)
      expect(hashes(items)).toEqual(['b', 'c', 'a']);
    });
  });

  // =========================================================
  // 7. updateLastAccessed 更新后重排并通知
  //    模拟"复制"操作：按 lastAccessed 排序时，
  //    更新第二条记录的 lastAccessed 应使其移到第一位
  // =========================================================
  describe('updateLastAccessed 触发重排和通知', () => {
    it('按 lastAccessed desc 排序时，updateLastAccessed 使记录移到首位', async () => {
      storage.setSortConfig({ field: 'lastAccessed', order: 'desc' });

      await storage.addItem(createItem('a', 100, { lastAccessed: 300 }));
      await storage.addItem(createItem('b', 200, { lastAccessed: 200 }));
      await storage.addItem(createItem('c', 300, { lastAccessed: 100 }));

      // 初始顺序: a(300) > b(200) > c(100)
      expect(hashes(await storage.getAllItems())).toEqual(['a', 'b', 'c']);

      // 「复制」c → lastAccessed 更新为最新
      await storage.updateLastAccessed('c');

      // c 应该移到首位
      const items = await storage.getAllItems();
      expect(items[0].profileHash).toBe('c');
      expect(items[0].lastAccessed).toBeGreaterThan(300);
    });

    it('updateLastAccessed 触发 update 通知', async () => {
      await storage.addItem(createItem('x', 100, { lastAccessed: 100 }));

      const callback = jest.fn();
      storage.addChangeCallback(callback);

      await storage.updateLastAccessed('x');

      // 刷新待处理通知
      await new Promise((r) => setTimeout(r, 150));

      expect(callback).toHaveBeenCalledWith(
        expect.arrayContaining([expect.objectContaining({ profileHash: 'x' })]),
        'update'
      );

      storage.removeChangeCallback(callback);
    });

    it('按 timestamp 排序时，updateLastAccessed 不改变顺序', async () => {
      // 默认 sortConfig 是 timestamp desc
      await storage.addItem(createItem('a', 100));
      await storage.addItem(createItem('b', 200));
      await storage.addItem(createItem('c', 300));

      // 初始顺序: c(300) > b(200) > a(100) by timestamp
      expect(hashes(await storage.getAllItems())).toEqual(['c', 'b', 'a']);

      await storage.updateLastAccessed('a');

      // timestamp 没变，顺序不变
      expect(hashes(await storage.getAllItems())).toEqual(['c', 'b', 'a']);
    });
  });
});
