/**
 * Clipboard Service Types
 * 剪贴板服务相关类型定义
 */

import { ClipboardContentType } from './api';

export type P2pDeliveryState = 'delivered' | 'partial' | 'offline' | 'failed' | 'pending';

export interface P2pDeliveryCounts {
  accepted: number;
  duplicate: number;
  offline: number;
  errored: number;
  pending: number;
}

export interface HistoryItemRevision {
  version: number;
  /** Local row identity. A physical deletion ends this identity. */
  incarnationId?: string;
}

/** Match both the row lifetime and its mutable version before deferred deletion. */
export function matchesHistoryRevision(
  item: HistoryItemRevision,
  expected: HistoryItemRevision | undefined
): boolean {
  return (
    expected !== undefined && item.version === expected.version &&
    item.incarnationId === expected.incarnationId
  );
}

/**
 * 历史记录同步状态
 */
export enum HistorySyncStatus {
  /** 仅本地存在，未同步到服务器 */
  LocalOnly = 0,

  /** 已与服务器同步 */
  Synced = 1,

  /** 需要同步（本地有未推送的变更） */
  NeedSync = 2,
}

/**
 * 剪贴板项目
 */
export interface ClipboardItem extends HistoryItemRevision {
  /** Process-local diagnostics only. The SQLite row mapper does not persist this field. */
  diagnosticReadId?: string;
  /** 内容类型 */
  type: ClipboardContentType;

  /** 文本内容（预览或完整） */
  text: string;

  /** Profile hash 值（遵循服务器规则） */
  profileHash: string;

  /** 是否有额外数据 */
  hasData: boolean;

  /** 数据文件名 */
  dataName?: string;

  /** 文件大小（字节） */
  size?: number;

  /** 创建时间戳 */
  timestamp: number;

  /** 设备名称 */
  deviceName?: string;

  /** 是否已同步（已废弃，使用 syncStatus） */
  synced?: boolean;

  /** 是否标记（收藏） */
  starred: boolean;

  /** 使用次数 */
  useCount?: number;

  /** 本地剪贴板 hash（用于本地变化检测，基于 base64 内容） */
  localClipboardHash?: string;

  /** 文件 URI（本地文件路径） */
  fileUri?: string;

  // === 同步相关字段 ===

  /** 同步状态 */
  syncStatus: HistorySyncStatus;

  /** 版本号（乐观锁） */
  version: number;

  /** 最后修改时间（UTC时间戳） */
  lastModified: number;

  /** 最后访问时间（UTC时间戳） */
  lastAccessed: number;

  /** 是否已删除（软删除标记） */
  isDeleted: boolean;

  /** 是否置顶 */
  pinned: boolean;

  /** 本地数据是否就绪（false表示仅有元数据） */
  isLocalFileReady: boolean;

  /** 来源设备 */
  from?: string;

  /** 远程是否有数据（服务器HasData字段） */
  hasRemoteData?: boolean;

  /**
   * 服务端身份（`blake3v1:<hex>`）。拉取/应用的远端条目回填此值，本地复制的条目为空。
   * 让每条历史都保留其服务端身份，activate_clipboard 借 profileHash 指针解析出 content_id。
   */
  contentId?: string;

  /** P2P 核心中的发送条目编号，用于失败后重新发送。 */
  p2pEntryId?: string;

  /** 最近一次 P2P 投递结果。 */
  p2pDeliveryState?: P2pDeliveryState;

  /** 最近一次 P2P 投递的各类设备数量。 */
  p2pDeliveryCounts?: P2pDeliveryCounts;
}

/**
 * 创建 ClipboardItem 的默认值
 */
export function createDefaultClipboardItem(
  base: Omit<
    ClipboardItem,
    | 'starred'
    | 'syncStatus'
    | 'version'
    | 'lastModified'
    | 'lastAccessed'
    | 'isDeleted'
    | 'pinned'
    | 'isLocalFileReady'
  > &
    Partial<
      Pick<
        ClipboardItem,
        | 'starred'
        | 'syncStatus'
        | 'version'
        | 'lastModified'
        | 'lastAccessed'
        | 'isDeleted'
        | 'pinned'
        | 'isLocalFileReady'
      >
    >
): ClipboardItem {
  const now = Date.now();
  return {
    starred: false,
    syncStatus: HistorySyncStatus.LocalOnly,
    version: 0,
    lastModified: now,
    lastAccessed: now,
    isDeleted: false,
    pinned: false,
    isLocalFileReady: true,
    ...base,
  };
}

/**
 * 剪贴板内容
 */
export interface ClipboardContent {
  /** Process-local identity for one file read. Never use it as content identity. */
  diagnosticReadId?: string;
  /** 内容类型 */
  type: ClipboardContentType;

  /** 文本内容 */
  text?: string;

  /** 文件 URI（本地文件路径） */
  fileUri?: string;

  /** 文件名 */
  fileName?: string;

  /** 文件大小 */
  fileSize?: number;

  /** Profile hash（用于服务器上传，遵循服务器规则） */
  profileHash?: string;

  /** 本地剪贴板 hash（用于本地变化检测，基于 base64 内容） */
  localClipboardHash?: string;

  /** 文件数据（二进制） */
  fileData?: ArrayBuffer;

  /** 创建时间戳 */
  timestamp?: number;

  /** 是否有额外数据文件（用于标识是否需要处理外部文件） */
  hasData?: boolean;
}

/**
 * 剪贴板监听器回调
 */
export type ClipboardChangeCallback = (content: ClipboardContent) => void;

/**
 * 剪贴板监听器选项
 */
export interface ClipboardMonitorOptions {
  /** 轮询间隔（毫秒），仅 iOS 使用 */
  pollingInterval?: number;

  /** 是否在应用进入后台时停止监听 */
  stopOnBackground?: boolean;

  /** 防抖延迟（毫秒） */
  debounceDelay?: number;
}

/**
 * 剪贴板历史项
 */
export interface ClipboardHistoryItem extends ClipboardItem {
  /** 备注 */
  note?: string;

  /** 使用次数 */
  useCount?: number;

  /** 最后使用时间 */
  lastUsed?: number;
}

/**
 * 剪贴板历史查询选项
 */
export interface ClipboardHistoryQuery {
  /** 类型筛选 */
  type?: ClipboardContentType;

  /** 搜索关键词 */
  keyword?: string;

  /** 开始时间 */
  startTime?: number;

  /** 结束时间 */
  endTime?: number;

  /** 是否只显示已标记 */
  starredOnly?: boolean;

  /** 跳过数量 */
  skip?: number;

  /** 限制数量 */
  limit?: number;

  /** 排序方式 */
  sortBy?: 'timestamp' | 'useCount' | 'lastUsed';

  /** 排序顺序 */
  sortOrder?: 'asc' | 'desc';
}
