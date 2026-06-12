/**
 * History API Types
 * 历史记录 API 相关类型定义
 */

import { ProgressInfo } from 'native-util';
import { ClipboardContentType } from '../types/api';
import { ClipboardItem, HistorySyncStatus } from '../types/clipboard';

/**
 * 历史记录 DTO（服务器格式）
 */
export interface HistoryRecordDto {
  hash: string;
  type: 'Text' | 'Image' | 'File';
  text?: string;
  createTime?: string;
  lastModified?: string;
  lastAccessed?: string;
  starred?: boolean;
  pinned?: boolean;
  size?: number;
  hasData?: boolean;
  version?: number;
  isDeleted?: boolean;
}

/**
 * 历史记录更新 DTO
 */
export interface HistoryRecordUpdateDto {
  starred?: boolean;
  pinned?: boolean;
  isDelete?: boolean;
  version?: number;
  lastModified?: string;
  lastAccessed?: string;
}

/**
 * 历史记录查询参数
 */
export interface HistoryQueryParams {
  page?: number;
  before?: string;
  after?: string;
  modifiedAfter?: string;
  types?: number;
  searchText?: string;
  starred?: boolean;
  sortByLastAccessed?: boolean;
}

/**
 * 历史记录统计 DTO
 */
export interface HistoryStatisticsDto {
  totalCount: number;
  starredCount: number;
  deletedCount: number;
  activeCount: number;
  totalFileSizeMB: number;
}

/**
 * 同步冲突错误
 */
export class SyncConflictError extends Error {
  public readonly serverRecord: HistoryRecordDto;

  constructor(message: string, serverRecord: HistoryRecordDto) {
    super(message);
    this.name = 'SyncConflictError';
    this.serverRecord = serverRecord;
  }
}

/**
 * 记录不存在错误
 */
export class RecordNotFoundError extends Error {
  public readonly profileId: string;

  constructor(profileId: string) {
    super(`Record not found: ${profileId}`);
    this.name = 'RecordNotFoundError';
    this.profileId = profileId;
  }
}

/**
 * 类型过滤位掩码
 */
export const ProfileTypeFilter = {
  Text: 1,
  Image: 2,
  File: 4,
  Group: 8,
  All: 15,
  FileAndGroup: 12,
} as const;

/**
 * History API 接口
 */
export interface IHistoryAPI {
  queryRecords(params: HistoryQueryParams, signal?: AbortSignal): Promise<HistoryRecordDto[]>;
  getRecord(profileId: string, signal?: AbortSignal): Promise<HistoryRecordDto>;
  updateRecord(
    type: 'Text' | 'Image' | 'File',
    profileId: string,
    update: HistoryRecordUpdateDto,
    signal?: AbortSignal
  ): Promise<HistoryRecordDto>;
  downloadData(
    profileId: string,
    destinationUri: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<string>;
  uploadRecord(
    record: HistoryRecordDto,
    fileUri?: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<HistoryRecordDto>;
  getStatistics(signal?: AbortSignal): Promise<HistoryStatisticsDto>;
  getServerTime(signal?: AbortSignal): Promise<Date>;
}

/**
 * 工具函数：将 HistoryRecordDto 转换为 ClipboardItem
 */
export function dtoToClipboardItem(dto: HistoryRecordDto): ClipboardItem {
  return {
    type: dto.type as ClipboardContentType,
    text: dto.text || '',
    profileHash: dto.hash,
    hasData: dto.hasData || false,
    size: dto.size ?? 0,
    timestamp: dto.createTime ? new Date(dto.createTime).getTime() : Date.now(),
    starred: dto.starred ?? false,
    pinned: dto.pinned ?? false,
    syncStatus: HistorySyncStatus.Synced,
    version: dto.version ?? 0,
    lastModified: dto.lastModified ? new Date(dto.lastModified).getTime() : Date.now(),
    lastAccessed: dto.lastAccessed ? new Date(dto.lastAccessed).getTime() : Date.now(),
    isDeleted: dto.isDeleted ?? false,
    hasRemoteData: dto.hasData ?? false,
    isLocalFileReady: false,
  };
}

/**
 * 工具函数：将 ClipboardItem 转换为 HistoryRecordDto
 */
export function clipboardItemToDto(item: ClipboardItem): HistoryRecordDto {
  const hash = item.profileHash.includes('-')
    ? item.profileHash.split('-').slice(1).join('-')
    : item.profileHash;

  return {
    hash,
    type: item.type as 'Text' | 'Image' | 'File',
    text: item.text,
    createTime: item.timestamp ? new Date(item.timestamp).toISOString() : undefined,
    lastModified: item.lastModified ? new Date(item.lastModified).toISOString() : undefined,
    lastAccessed: item.lastAccessed ? new Date(item.lastAccessed).toISOString() : undefined,
    starred: item.starred,
    pinned: item.pinned,
    size: item.size,
    hasData: item.hasData,
    version: item.version,
    isDeleted: item.isDeleted,
  };
}

/**
 * 工具函数：生成 profileId
 */
export function getProfileId(type: string, hash: string): string {
  return `${type}-${hash}`;
}

/**
 * 工具函数：从 profileId 解析 type 和 hash
 */
export function parseProfileId(
  profileId: string
): { type: 'Text' | 'Image' | 'File'; hash: string } | null {
  const parts = profileId.split('-');
  if (parts.length < 2) {
    return null;
  }
  const type = parts[0] as 'Text' | 'Image' | 'File';
  const hash = parts.slice(1).join('-');
  if (!['Text', 'Image', 'File'].includes(type)) {
    return null;
  }
  return { type, hash };
}
