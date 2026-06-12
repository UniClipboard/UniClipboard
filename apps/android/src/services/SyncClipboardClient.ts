/**
 * SyncClipboard API Client
 * Implements SyncClipboard server API operations
 */

import {
  nativeDownloadFile,
  nativeUploadFile,
  nativeUploadMultipart,
  ProgressInfo,
} from 'native-util';
import { APIClient, APIClientConfig, PutContentOptions, ISyncClipboardAPI } from './APIClient';
import { ProfileDto, ServerInfo } from '../types/api';
import type { ClipboardContent } from '../types/clipboard';
import { ValidationError, ServerError } from './errors';
import { isTextInvalid } from '../utils/index';
import {
  HistoryRecordDto,
  HistoryRecordUpdateDto,
  HistoryQueryParams,
  HistoryStatisticsDto,
  SyncConflictError,
  RecordNotFoundError,
  IHistoryAPI,
} from './HistoryAPI';

/**
 * SyncClipboard API 客户端
 * 融合了剪贴板操作和历史记录操作
 */
export class SyncClipboardClient extends APIClient implements ISyncClipboardAPI, IHistoryAPI {
  private static readonly PROFILE_ENDPOINT = '/SyncClipboard.json';
  private static readonly HISTORY_API_PREFIX = '/api/history';

  constructor(config: APIClientConfig) {
    super(config);
  }

  /**
   * 获取剪贴板配置
   */
  async getClipboard(signal?: AbortSignal): Promise<ProfileDto> {
    try {
      const profile = await this.get<ProfileDto>(
        SyncClipboardClient.PROFILE_ENDPOINT,
        signal ? { signal } : undefined
      );

      this.validateProfile(profile);

      return profile;
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to get clipboard:', error);
      throw error;
    }
  }

  /**
   * 上传剪贴板配置
   */
  async putClipboard(profile: ProfileDto, signal?: AbortSignal): Promise<void> {
    try {
      this.validateProfile(profile);

      console.log(
        '[SyncClipboardClient] putClipboard - Profile to upload:',
        JSON.stringify(profile, null, 2)
      );

      await this.put(
        SyncClipboardClient.PROFILE_ENDPOINT,
        profile,
        signal ? { signal } : undefined
      );

      console.log('[SyncClipboardClient] putClipboard - Upload successful');
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to put clipboard:', error);
      if (error instanceof Error) {
        console.error('[SyncClipboardClient] Error details:', {
          message: error.message,
          name: error.name,
        });
      }
      if (error && typeof error === 'object' && 'response' in error) {
        const errorObj = error as Record<string, unknown>;
        console.error(
          '[SyncClipboardClient] Server response:',
          JSON.stringify(errorObj.response, null, 2)
        );
      }
      throw error;
    }
  }

  /**
   * 上传文件数据
   */
  async putFile(
    fileName: string,
    fileUri: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<void> {
    if (!fileName) {
      throw new ValidationError('File name is required');
    }

    if (!fileUri) {
      throw new ValidationError('File URI is required');
    }

    console.log(`[SyncClipboardClient] Uploading file: ${fileName}`);

    const url = `${this.baseURL}/file/${encodeURIComponent(fileName)}`;

    const headers = await this.getHeaders();
    headers['Content-Type'] = 'application/octet-stream';

    try {
      await nativeUploadFile(url, headers, fileUri, signal, onProgress);
      console.log(`[SyncClipboardClient] File uploaded successfully: ${fileName}`);
    } catch (error) {
      console.error(`[SyncClipboardClient] Failed to put file ${fileName}:`, error);
      throw error;
    }
  }

  /**
   * 上传剪贴板内容（重写父类方法，与桌面端一致）
   * 1. 先检查服务器是否已存在该历史记录
   * 2. 如果不存在，上传到历史记录 API
   * 3. 最后上传当前剪贴板配置
   */
  async putContent(content: ClipboardContent, options?: PutContentOptions): Promise<void> {
    const signal = options?.signal;
    const onProgress = options?.onProgress;

    console.log('[SyncClipboardClient] putContent - Starting:', {
      type: content.type,
      hasData: content.hasData,
      fileName: content.fileName,
    });

    const { contentToProfileDto } = await import('../utils/clipboard');
    const profile = await contentToProfileDto(content, { signal });

    if (!profile.hash) {
      throw new ValidationError('Profile hash is required for history upload');
    }

    const profileId = `${profile.type}-${profile.hash}`;

    let existingRecord: HistoryRecordDto | null = null;
    try {
      existingRecord = await this.getRecord(profileId, signal);
      if (existingRecord.isDeleted) {
        existingRecord = null;
      }
    } catch (error) {
      if (error instanceof RecordNotFoundError) {
        existingRecord = null;
      } else {
        throw error;
      }
    }

    if (existingRecord && !existingRecord.isDeleted) {
      console.log(
        '[SyncClipboardClient] History record already exists on server, skipping data upload'
      );
    } else {
      if (existingRecord?.isDeleted) {
        console.log('[SyncClipboardClient] History record was deleted, re-uploading');
      }
      const record: HistoryRecordDto = {
        hash: profile.hash,
        type: profile.type as 'Text' | 'Image' | 'File',
        text: profile.text,
        hasData: profile.hasData,
        size: profile.size,
        createTime: new Date().toISOString(),
        lastModified: new Date().toISOString(),
        lastAccessed: new Date().toISOString(),
        starred: false,
        pinned: false,
        version: 0,
        isDeleted: false,
      };

      const fileUri = content.fileUri || undefined;

      try {
        await this.uploadRecord(record, fileUri, signal, onProgress);
        console.log('[SyncClipboardClient] History record uploaded successfully');
      } catch (error) {
        if (error instanceof SyncConflictError) {
          console.log('[SyncClipboardClient] History record conflict, server already has it');
        } else {
          throw error;
        }
      }
    }

    await this.putClipboard(profile, signal);

    console.log('[SyncClipboardClient] putContent completed successfully');
  }

  /**
   * 获取服务器时间
   */
  async getServerTime(signal?: AbortSignal): Promise<Date> {
    const response = await this.get<string>('/api/time', { signal });
    return new Date(response);
  }

  /**
   * 获取服务器版本
   */
  async getVersion(): Promise<string> {
    try {
      const version = await this.get<string>('/version').catch(() => 'Unknown');
      return version;
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to get version:', error);
      return 'Unknown';
    }
  }

  /**
   * 获取服务器信息
   */
  async getServerInfo(): Promise<ServerInfo> {
    try {
      const [version, serverTime] = await Promise.all([this.getVersion(), this.getServerTime()]);

      return {
        version,
        serverTime,
        online: true,
      };
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to get server info:', error);
      return {
        version: 'Unknown',
        serverTime: new Date(),
        online: false,
      };
    }
  }

  /**
   * 测试 API 连接
   */
  async testConnection(signal?: AbortSignal): Promise<void> {
    const serverTime = await this.getServerTime(signal);
    const localTime = new Date();
    const timeDiffMs = Math.abs(localTime.getTime() - serverTime.getTime());
    const timeDiffMinutes = timeDiffMs / (1000 * 60);

    if (timeDiffMinutes > 5) {
      throw new Error(
        `服务器时间与本地时间差距过大（${Math.round(timeDiffMinutes)}分钟），请同步系统时间`
      );
    }
  }

  /**
   * 查询历史记录 (IHistoryAPI)
   */
  async queryRecords(
    params: HistoryQueryParams,
    signal?: AbortSignal
  ): Promise<HistoryRecordDto[]> {
    const formData = new FormData();

    if (params.page !== undefined) {
      formData.append('page', params.page.toString());
    }
    if (params.before) {
      formData.append('before', params.before);
    }
    if (params.after) {
      formData.append('after', params.after);
    }
    if (params.modifiedAfter) {
      formData.append('modifiedAfter', params.modifiedAfter);
    }
    if (params.types !== undefined) {
      formData.append('types', params.types.toString());
    }
    if (params.searchText) {
      formData.append('searchText', params.searchText);
    }
    if (params.starred !== undefined) {
      formData.append('starred', params.starred.toString());
    }
    if (params.sortByLastAccessed !== undefined) {
      formData.append('sortByLastAccessed', params.sortByLastAccessed.toString());
    }

    console.log(
      '[SyncClipboardClient] queryRecords params:',
      Object.fromEntries(formData as unknown as Iterable<[string, string]>)
    );

    try {
      const response = await this.client.post<HistoryRecordDto[]>(
        `${SyncClipboardClient.HISTORY_API_PREFIX}/query`,
        formData,
        {
          headers: {
            'Content-Type': 'multipart/form-data',
          },
          signal,
        }
      );
      console.log(
        '[SyncClipboardClient] queryRecords response length:',
        response.data?.length || 0
      );
      return response.data || [];
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to query records:', error);
      throw error;
    }
  }

  /**
   * 获取单条历史记录 (IHistoryAPI)
   */
  async getRecord(profileId: string, signal?: AbortSignal): Promise<HistoryRecordDto> {
    if (!profileId) {
      throw new ValidationError('Profile ID is required');
    }

    try {
      const record = await this.get<HistoryRecordDto>(
        `${SyncClipboardClient.HISTORY_API_PREFIX}/${encodeURIComponent(profileId)}`,
        signal ? { signal } : undefined
      );
      return record;
    } catch (error) {
      if (error instanceof ServerError && error.statusCode === 404) {
        throw new RecordNotFoundError(profileId);
      }
      console.error('[SyncClipboardClient] Failed to get record:', error);
      throw error;
    }
  }

  /**
   * 更新历史记录 (IHistoryAPI)
   */
  async updateRecord(
    type: 'Text' | 'Image' | 'File',
    profileId: string,
    update: HistoryRecordUpdateDto,
    signal?: AbortSignal
  ): Promise<HistoryRecordDto> {
    if (!profileId) {
      throw new ValidationError('Profile ID is required');
    }

    try {
      const record = await this.patch<HistoryRecordDto>(
        `${SyncClipboardClient.HISTORY_API_PREFIX}/${type}/${encodeURIComponent(profileId)}`,
        update,
        signal ? { signal } : undefined
      );
      return record;
    } catch (error) {
      if (error && typeof error === 'object') {
        const apiError = error as { statusCode?: number; response?: unknown };

        if (apiError.statusCode === 409 && apiError.response) {
          const serverRecord = apiError.response as HistoryRecordDto;
          throw new SyncConflictError('Version conflict', serverRecord);
        }

        if (apiError.statusCode === 404) {
          throw new RecordNotFoundError(profileId);
        }
      }
      console.error('[SyncClipboardClient] Failed to update record:', error);
      throw error;
    }
  }

  /**
   * 下载历史记录数据文件 (IHistoryAPI)
   */
  async downloadData(
    profileId: string,
    destinationUri: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<string> {
    if (!profileId) {
      throw new ValidationError('Profile ID is required');
    }
    if (!destinationUri) {
      throw new ValidationError('Destination URI is required');
    }

    const url = `${this.baseURL}${SyncClipboardClient.HISTORY_API_PREFIX}/${encodeURIComponent(profileId)}/data`;

    try {
      const headers = await this.getHeaders();

      console.log(`[SyncClipboardClient] Downloading data: ${profileId}`);

      await nativeDownloadFile(url, headers, destinationUri, signal, onProgress);

      console.log(`[SyncClipboardClient] Data downloaded successfully: ${profileId}`);
      return destinationUri;
    } catch (error) {
      console.error(`[SyncClipboardClient] Failed to download data:`, error);
      throw error;
    }
  }

  /**
   * 上传历史记录 (IHistoryAPI)
   */
  async uploadRecord(
    record: HistoryRecordDto,
    fileUri?: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<HistoryRecordDto> {
    if (!record.hash) {
      throw new ValidationError('Record hash is required');
    }
    if (!record.type) {
      throw new ValidationError('Record type is required');
    }

    const url = `${this.baseURL}${SyncClipboardClient.HISTORY_API_PREFIX}`;
    const headers = await this.getHeaders();

    const formFields: Record<string, string> = {
      hash: record.hash,
      type: record.type,
    };

    if (!isTextInvalid(record.text)) {
      formFields.text = record.text;
    }
    if (record.createTime) {
      formFields.createTime = record.createTime;
    }
    if (record.lastModified) {
      formFields.lastModified = record.lastModified;
    }
    if (record.lastAccessed) {
      formFields.lastAccessed = record.lastAccessed;
    }
    if (record.starred !== undefined) {
      formFields.starred = record.starred.toString();
    }
    if (record.pinned !== undefined) {
      formFields.pinned = record.pinned.toString();
    }
    if (record.size !== undefined) {
      formFields.size = record.size.toString();
    }
    if (record.hasData !== undefined) {
      formFields.hasData = record.hasData.toString();
    }
    if (record.isDeleted !== undefined) {
      formFields.isDeleted = record.isDeleted.toString();
    }
    if (record.version !== undefined) {
      formFields.version = record.version.toString();
    }

    console.log('[SyncClipboardClient] Uploading history record:', {
      hash: record.hash,
      type: record.type,
      hasFile: !!fileUri,
    });

    try {
      await nativeUploadMultipart(
        url,
        headers,
        formFields,
        fileUri || undefined,
        signal,
        onProgress
      );
      console.log('[SyncClipboardClient] History record uploaded successfully');
      return this.getRecord(`${record.type}-${record.hash}`, signal);
    } catch (error) {
      if (error && typeof error === 'object' && 'statusCode' in error) {
        const errorObj = error as { statusCode: number; response?: unknown };
        if (errorObj.statusCode === 409) {
          const serverRecord = errorObj.response as HistoryRecordDto;
          throw new SyncConflictError('History record already exists', serverRecord);
        }
      }
      console.error('[SyncClipboardClient] Failed to upload history record:', error);
      throw error;
    }
  }

  /**
   * 获取历史记录统计信息 (IHistoryAPI)
   */
  async getStatistics(signal?: AbortSignal): Promise<HistoryStatisticsDto> {
    try {
      const stats = await this.get<HistoryStatisticsDto>(
        `${SyncClipboardClient.HISTORY_API_PREFIX}/statistics`,
        signal ? { signal } : undefined
      );
      return stats;
    } catch (error) {
      console.error('[SyncClipboardClient] Failed to get statistics:', error);
      throw error;
    }
  }

  /**
   * 验证 ProfileDto 数据
   */
  private validateProfile(profile: ProfileDto): void {
    if (!profile) {
      throw new ValidationError('Profile is required');
    }

    if (!profile.type) {
      throw new ValidationError('Profile type is required');
    }

    const validTypes = ['Text', 'Image', 'File', 'Group'];
    if (!validTypes.includes(profile.type)) {
      throw new ValidationError(`Invalid profile type: ${profile.type}`);
    }

    if (typeof profile.text !== 'string') {
      throw new ValidationError('Profile text must be a string');
    }

    if (typeof profile.hasData !== 'boolean') {
      throw new ValidationError('Profile hasData must be a boolean');
    }

    if (profile.hasData && !profile.dataName) {
      throw new ValidationError('Profile dataName is required when hasData is true');
    }
  }
}
