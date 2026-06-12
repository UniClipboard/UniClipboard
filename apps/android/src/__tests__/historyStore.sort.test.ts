/**
 * historyStore handleStorageChange 排序测试
 * 验证 update 事件到达 store 后，items 列表重排
 */

import { useHistoryStore } from '../stores/historyStore';
import { ClipboardItem, HistorySyncStatus } from '../types/clipboard';

// Mock historyStorage，避免真实 AsyncStorage 依赖
jest.mock('../services', () => ({
  historyStorage: {
    searchItems: jest.fn().mockResolvedValue({ items: [], total: 0 }),
    addItem: jest.fn(),
    addItems: jest.fn(),
    updateItem: jest.fn(),
    softDeleteItem: jest.fn(),
    softDeleteItems: jest.fn(),
    toggleStar: jest.fn(),
    togglePin: jest.fn(),
    incrementUseCount: jest.fn(),
    clear: jest.fn(),
    setSortConfig: jest.fn(),
  },
}));

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

function hashes(items: ClipboardItem[]): string[] {
  return items.map((i) => i.profileHash);
}

describe('historyStore handleStorageChange 排序', () => {
  beforeEach(() => {
    useHistoryStore.getState().reset();
  });

  it('update 事件中 lastAccessed 变化时，删除+按序插入使记录移到正确位置', () => {
    const { handleStorageChange, setSort } = useHistoryStore.getState();

    // 设置排序为 lastAccessed desc
    setSort({ field: 'lastAccessed', order: 'desc' });

    // 直接设置初始 items（模拟已加载状态）
    useHistoryStore.setState({
      items: [
        createItem('a', 100, { lastAccessed: 300 }),
        createItem('b', 200, { lastAccessed: 200 }),
        createItem('c', 300, { lastAccessed: 100 }),
      ],
      totalCount: 3,
    });

    // 模拟 c 的 lastAccessed 更新为 999（复制操作）
    handleStorageChange([createItem('c', 300, { lastAccessed: 999 })], 'update');

    const items = useHistoryStore.getState().items;
    // c 应该移到首位
    expect(hashes(items)).toEqual(['c', 'a', 'b']);
  });

  it('update 事件中 pinned 变化时，删除+按序插入使记录移到 pinned 区域', () => {
    const { handleStorageChange, setSort } = useHistoryStore.getState();

    setSort({ field: 'timestamp', order: 'desc' });

    useHistoryStore.setState({
      items: [createItem('c', 300), createItem('b', 200), createItem('a', 100)],
      totalCount: 3,
    });

    // a 被置顶
    handleStorageChange([createItem('a', 100, { pinned: true })], 'update');

    const items = useHistoryStore.getState().items;
    // pinned 的 a 在最前，其余按 timestamp desc
    expect(hashes(items)).toEqual(['a', 'c', 'b']);
  });

  it('update 事件中排序字段没变时不重排', () => {
    const { handleStorageChange, setSort } = useHistoryStore.getState();

    setSort({ field: 'timestamp', order: 'desc' });

    useHistoryStore.setState({
      items: [createItem('c', 300), createItem('b', 200), createItem('a', 100)],
      totalCount: 3,
    });

    // 只更新 text，不影响排序字段
    handleStorageChange([{ ...createItem('a', 100), text: 'updated text' }], 'update');

    const items = useHistoryStore.getState().items;
    // 顺序不变
    expect(hashes(items)).toEqual(['c', 'b', 'a']);
    // 但内容已更新
    expect(items[2].text).toBe('updated text');
  });

  it('searchItems 不传 sort 时不覆盖已有的 sort 配置', async () => {
    const { setSort, searchItems } = useHistoryStore.getState();

    // 先设置排序为 lastAccessed desc（模拟 loadSortSetting 完成）
    setSort({ field: 'lastAccessed', order: 'desc' });

    // searchItems 只传 filter，不传 sort（模拟防抖 useEffect 搜索）
    await searchItems({ keyword: 'test' });

    // sort 不应该被覆盖
    const sort = useHistoryStore.getState().sort;
    expect(sort).toEqual({ field: 'lastAccessed', order: 'desc' });
  });

  it('searchItems 不传 sort 时，handleStorageChange update 仍按正确 sort 重排', async () => {
    const { setSort, searchItems, handleStorageChange } = useHistoryStore.getState();

    // 设置排序为 lastAccessed desc
    setSort({ field: 'lastAccessed', order: 'desc' });

    // searchItems 只传 filter 不传 sort（模拟初始加载）
    await searchItems(undefined);

    // 手动设置 items（模拟数据已加载）
    useHistoryStore.setState({
      items: [
        createItem('a', 100, { lastAccessed: 300 }),
        createItem('b', 200, { lastAccessed: 200 }),
        createItem('c', 300, { lastAccessed: 100 }),
      ],
      totalCount: 3,
    });

    // update c 的 lastAccessed
    handleStorageChange([createItem('c', 300, { lastAccessed: 999 })], 'update');

    const items = useHistoryStore.getState().items;
    // sort 应该还是 lastAccessed desc，c 移到首位
    expect(hashes(items)).toEqual(['c', 'a', 'b']);
  });
});
