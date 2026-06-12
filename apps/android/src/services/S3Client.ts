/**
 * S3 Client
 * Implements SyncClipboard API using S3-compatible object storage
 * Uses AWS Signature V4 for authentication
 */

import axios, { AxiosInstance } from 'axios';
import { sha256 } from 'js-sha256';
import {
  nativeUploadFile,
  nativeDownloadFile,
  nativeCalculateStringMD5Base64,
  nativeCalculateFileMD5Base64,
  type ProgressInfo,
} from 'native-util';
import { ISyncClipboardAPI, PutContentOptions, DownloadProgressCallback } from './APIClient';
import { ProfileDto, ServerInfo } from '../types/api';
import type { ClipboardContent } from '../types/clipboard';
import { ValidationError, ServerError, NetworkError, TimeoutError, APIError } from './errors';
import { APP_NAME, APP_VERSION } from '../constants';

/**
 * S3 客户端配置
 */
export interface S3ClientConfig {
  /** S3 兼容端点 URL（AWS 原生留空） */
  serviceURL?: string;
  /** AWS 区域 */
  region?: string;
  /** 存储桶名称 */
  bucketName: string;
  /** 对象 key 前缀 */
  objectPrefix?: string;
  /** 是否使用路径风格寻址 */
  forcePathStyle?: boolean;
  /** Access Key ID */
  accessKeyId: string;
  /** Secret Access Key */
  secretAccessKey: string;
  /** 超时时间（毫秒） */
  timeout?: number;
}

/**
 * 已解析的端点信息
 */
interface ResolvedEndpoint {
  /** 完整基础 URL（含 bucket） */
  baseURL: string;
  /** 请求的 Host 头 */
  host: string;
  /** 路径前缀（path-style 时为 /{bucket}，否则为空） */
  bucketPath: string;
  /** 是否使用 HTTPS */
  useHttps: boolean;
}

const UNSIGNED_PAYLOAD = 'UNSIGNED-PAYLOAD';
const PROFILE_FILENAME = 'SyncClipboard.json';
const FILE_FOLDER = 'file';

/**
 * S3 兼容对象存储客户端
 * 使用 AWS Signature V4 认证
 */
export class S3Client implements ISyncClipboardAPI {
  private client: AxiosInstance;
  private endpoint: ResolvedEndpoint;
  private region: string;
  private bucketName: string;
  private objectPrefix: string;
  private accessKeyId: string;
  private secretAccessKey: string;
  private isCustomEndpoint: boolean;

  constructor(config: S3ClientConfig) {
    this.validateConfig(config);

    this.region = (config.region || 'us-east-1').trim();
    this.bucketName = config.bucketName.trim();
    this.objectPrefix = (config.objectPrefix || '').replace(/^\/+|\/+$/g, '');
    this.accessKeyId = config.accessKeyId.trim();
    this.secretAccessKey = config.secretAccessKey;
    this.isCustomEndpoint = !!config.serviceURL?.trim();

    this.endpoint = this.resolveEndpoint(config);

    this.client = axios.create({
      baseURL: this.endpoint.baseURL,
      timeout: config.timeout || 30000,
      // S3 返回 XML，不自动解析
      transformResponse: (data) => data,
      headers: {
        'User-Agent': `${APP_NAME}/${APP_VERSION}`,
      },
    });
  }

  // ─── ISyncClipboardAPI 实现 ────────────────────────────────────

  /**
   * 获取剪贴板配置
   */
  async getClipboard(signal?: AbortSignal): Promise<ProfileDto> {
    const key = this.buildObjectKey(PROFILE_FILENAME);
    const path = `${this.endpoint.bucketPath}/${key}`;

    try {
      const headers = this.signRequest('GET', path, {}, '', signal);
      const response = await this.client.get(`/${key}`, { headers, signal });

      const profile: ProfileDto =
        typeof response.data === 'string' ? JSON.parse(response.data) : response.data;
      this.validateProfile(profile);
      return profile;
    } catch (error) {
      // 404 表示远程剪贴板为空（首次连接），返回默认空配置
      if (axios.isAxiosError(error) && error.response?.status === 404) {
        return { type: 'Text', text: '', hasData: false } as ProfileDto;
      }
      console.error('[S3Client] Failed to get clipboard:', error);
      throw this.wrapError(error);
    }
  }

  /**
   * 上传剪贴板配置
   */
  async putClipboard(profile: ProfileDto, signal?: AbortSignal): Promise<void> {
    this.validateProfile(profile);

    const key = this.buildObjectKey(PROFILE_FILENAME);
    const path = `${this.endpoint.bucketPath}/${key}`;
    const body = JSON.stringify(profile);

    try {
      const contentMD5 = nativeCalculateStringMD5Base64(body);
      const extraHeaders: Record<string, string> = {
        'Content-Type': 'application/json; charset=utf-8',
        'Content-MD5': contentMD5,
      };
      const headers = this.signRequest('PUT', path, extraHeaders, body, signal);
      await this.client.put(`/${key}`, body, { headers, signal });
    } catch (error) {
      console.error('[S3Client] Failed to put clipboard:', error);
      throw this.wrapError(error);
    }
  }

  /**
   * 下载文件到指定路径
   */
  async downloadFile(
    fileName: string,
    destinationUri: string,
    signal?: AbortSignal,
    onProgress?: DownloadProgressCallback
  ): Promise<string> {
    if (!fileName) throw new ValidationError('File name is required');
    if (!destinationUri) throw new ValidationError('Destination URI is required');

    const key = this.buildObjectKey(`${FILE_FOLDER}/${fileName}`);
    const path = `${this.endpoint.bucketPath}/${key}`;
    const url = `${this.endpoint.baseURL}/${key}`;

    try {
      const headers = this.signRequest('GET', path, {}, '', signal);
      console.log(`[S3Client] Downloading file ${fileName} to ${destinationUri}`);
      await nativeDownloadFile(url, headers, destinationUri, signal, onProgress);
      console.log(`[S3Client] File downloaded successfully: ${fileName}`);
      return destinationUri;
    } catch (error) {
      console.error(`[S3Client] Failed to download file ${fileName}:`, error);
      throw this.wrapError(error);
    }
  }

  /**
   * 上传文件
   */
  async putFile(
    fileName: string,
    fileUri: string,
    signal?: AbortSignal,
    onProgress?: (info: ProgressInfo) => void
  ): Promise<void> {
    if (!fileName) throw new ValidationError('File name is required');
    if (!fileUri) throw new ValidationError('File URI is required');

    const key = this.buildObjectKey(`${FILE_FOLDER}/${fileName}`);
    const path = `${this.endpoint.bucketPath}/${key}`;
    const url = `${this.endpoint.baseURL}/${key}`;

    try {
      const contentMD5 = await nativeCalculateFileMD5Base64(fileUri, signal);
      const extraHeaders: Record<string, string> = {
        'Content-Type': 'application/octet-stream',
        'Content-MD5': contentMD5,
      };
      const headers = this.signRequest('PUT', path, extraHeaders, '', signal);
      console.log(`[S3Client] Uploading file: ${fileName}`);
      await nativeUploadFile(url, headers, fileUri, signal, onProgress);
      console.log(`[S3Client] File uploaded successfully: ${fileName}`);
    } catch (error) {
      console.error(`[S3Client] Failed to put file ${fileName}:`, error);
      throw this.wrapError(error);
    }
  }

  /**
   * 上传剪贴板内容（先上传文件，再上传配置）
   */
  async putContent(content: ClipboardContent, options?: PutContentOptions): Promise<void> {
    try {
      console.log('[S3Client] Starting putContent:', {
        type: content.type,
        hasData: content.hasData,
        fileName: content.fileName,
      });

      const { contentToProfileDto } = await import('../utils/clipboard');
      const profile = await contentToProfileDto(content, { signal: options?.signal });

      // 确保 file/ 目录标记存在
      await this.ensureFileFolderExists(options?.signal);

      if (profile.hasData && profile.dataName && content.fileUri) {
        console.log(`[S3Client] Uploading data file: ${profile.dataName}`);
        await this.putFile(profile.dataName, content.fileUri, options?.signal, options?.onProgress);
      }

      console.log('[S3Client] Uploading profile...');
      await this.putClipboard(profile, options?.signal);
      console.log('[S3Client] putContent completed successfully');
    } catch (error) {
      console.error('[S3Client] Failed to put content:', error);
      throw error;
    }
  }

  /**
   * 获取服务器时间（S3 从响应头 Date 获取）
   */
  async getServerTime(signal?: AbortSignal): Promise<Date> {
    try {
      const key = this.buildObjectKey('');
      const path = `${this.endpoint.bucketPath}/${key}`.replace(/\/+$/, '/') || '/';
      const query: Record<string, string> = {
        'list-type': '2',
        prefix: key ? `${key}/` : '',
        'max-keys': '1',
      };

      const headers = this.signRequest('GET', path, {}, '', signal, query);
      const queryString = Object.entries(query)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join('&');

      const response = await this.client.get(`/${key}?${queryString}`, { headers, signal });
      const dateHeader = response.headers['date'];
      return dateHeader ? new Date(dateHeader) : new Date();
    } catch {
      return new Date();
    }
  }

  /**
   * 获取服务器版本
   */
  async getVersion(): Promise<string> {
    return 'S3 Compatible Storage';
  }

  /**
   * 获取服务器信息
   */
  async getServerInfo(): Promise<ServerInfo> {
    try {
      const serverTime = await this.getServerTime();
      return {
        version: 'S3 Compatible Storage',
        serverTime,
        online: true,
      };
    } catch {
      return {
        version: 'S3 Compatible Storage',
        serverTime: new Date(),
        online: false,
      };
    }
  }

  /**
   * 测试连接（ListObjectsV2）
   */
  async testConnection(signal?: AbortSignal): Promise<void> {
    const prefix = this.objectPrefix ? `${this.objectPrefix}/` : '';
    // path-style: /{bucket}/ ; virtual-host: /
    const path = this.endpoint.bucketPath ? `${this.endpoint.bucketPath}/` : '/';
    const query: Record<string, string> = {
      'list-type': '2',
      'max-keys': '1',
    };
    if (prefix) {
      query['prefix'] = prefix;
    }

    try {
      const headers = this.signRequest('GET', path, {}, '', signal, query);
      const queryString = Object.entries(query)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join('&');

      await this.client.get(`/?${queryString}`, { headers, signal });
      console.log('[S3Client] Connection test succeeded');
    } catch (error) {
      if (axios.isAxiosError(error) && error.response) {
        console.error(
          '[S3Client] Connection test failed:',
          error.response.status,
          error.response.data
        );
      } else {
        console.error('[S3Client] Connection test failed:', error);
      }
      throw this.wrapError(error);
    }
  }

  // ─── S3 内部方法 ──────────────────────────────────────────────

  /**
   * 确保 file/ 目录标记存在
   */
  private async ensureFileFolderExists(signal?: AbortSignal): Promise<void> {
    const markerKey = this.buildObjectKey(`${FILE_FOLDER}/`);
    const path = `${this.endpoint.bucketPath}/${markerKey}`;

    try {
      // HEAD 检查目录标记是否存在
      const headers = this.signRequest('HEAD', path, {}, '', signal);
      await this.client.head(`/${markerKey}`, { headers, signal });
    } catch {
      // 不存在则创建
      try {
        const contentMD5 = nativeCalculateStringMD5Base64('');
        const extraHeaders: Record<string, string> = {
          'Content-Type': 'application/x-directory',
          'Content-MD5': contentMD5,
        };
        const headers = this.signRequest('PUT', path, extraHeaders, '', signal);
        await this.client.put(`/${markerKey}`, '', { headers, signal });
      } catch (putError) {
        console.warn('[S3Client] Failed to create file folder marker:', putError);
      }
    }
  }

  /**
   * 构建对象 Key（含前缀）
   */
  private buildObjectKey(relativePath: string): string {
    const normalized = relativePath.replace(/\\/g, '/').replace(/^\/+/, '');
    if (!this.objectPrefix) return normalized;
    if (!normalized) return this.objectPrefix;
    return `${this.objectPrefix}/${normalized}`;
  }

  /**
   * 解析 S3 端点
   */
  private resolveEndpoint(config: S3ClientConfig): ResolvedEndpoint {
    const serviceURL = config.serviceURL?.trim().replace(/\/+$/, '');
    const bucket = config.bucketName.trim();
    const region = (config.region || 'us-east-1').trim();
    const forcePathStyle = config.forcePathStyle ?? false;

    if (serviceURL) {
      // 自定义端点
      const url = new URL(serviceURL);
      const useHttps = url.protocol === 'https:';

      if (forcePathStyle) {
        return {
          baseURL: `${serviceURL}/${bucket}`,
          host: url.host,
          bucketPath: `/${bucket}`,
          useHttps,
        };
      } else {
        const host = `${bucket}.${url.host}`;
        return {
          baseURL: `${url.protocol}//${host}`,
          host,
          bucketPath: '',
          useHttps,
        };
      }
    } else {
      // AWS 原生端点
      const host = `${bucket}.s3.${region}.amazonaws.com`;
      return {
        baseURL: `https://${host}`,
        host,
        bucketPath: '',
        useHttps: true,
      };
    }
  }

  // ─── AWS Signature V4 ────────────────────────────────────────

  /**
   * 对请求进行 AWS Signature V4 签名
   * 返回包含 Authorization 和其他必要头的 headers 对象
   */
  private signRequest(
    method: string,
    canonicalPath: string,
    extraHeaders: Record<string, string>,
    payload: string,
    _signal?: AbortSignal,
    queryParams?: Record<string, string>
  ): Record<string, string> {
    const now = new Date();
    const amzDate = this.formatAmzDate(now);
    const dateStamp = amzDate.substring(0, 8);

    // 构建要签名的 headers
    const headers: Record<string, string> = {
      host: this.endpoint.host,
      'x-amz-date': amzDate,
      'x-amz-content-sha256': UNSIGNED_PAYLOAD,
      ...extraHeaders,
    };

    // 1. 构建 Canonical Request
    const canonicalUri = this.encodeCanonicalUri(canonicalPath || '/');
    const canonicalQueryString = this.buildCanonicalQueryString(queryParams || {});
    const { canonicalHeaders, signedHeaders } = this.buildCanonicalHeaders(headers);
    const payloadHash = UNSIGNED_PAYLOAD;

    const canonicalRequest = [
      method.toUpperCase(),
      canonicalUri,
      canonicalQueryString,
      canonicalHeaders,
      signedHeaders,
      payloadHash,
    ].join('\n');

    console.debug(
      '[S3Client] Canonical Request:',
      JSON.stringify({
        method: method.toUpperCase(),
        canonicalUri,
        canonicalQueryString,
        signedHeaders,
        host: headers.host,
        region: this.region,
      })
    );

    // 2. 构建 String to Sign
    const credentialScope = `${dateStamp}/${this.region}/s3/aws4_request`;
    const stringToSign = [
      'AWS4-HMAC-SHA256',
      amzDate,
      credentialScope,
      sha256(canonicalRequest),
    ].join('\n');

    // 3. 计算签名
    const signingKey = this.deriveSigningKey(dateStamp);
    const signature = sha256.hmac(signingKey, stringToSign);

    // 4. 构建 Authorization 头
    headers['Authorization'] =
      `AWS4-HMAC-SHA256 Credential=${this.accessKeyId}/${credentialScope}, ` +
      `SignedHeaders=${signedHeaders}, ` +
      `Signature=${signature}`;

    // 移除 host 头（由 HTTP 客户端自动添加）
    const result: Record<string, string> = {};
    for (const [key, value] of Object.entries(headers)) {
      if (key.toLowerCase() !== 'host') {
        result[key] = value;
      }
    }
    return result;
  }

  /**
   * 派生 AWS Sig V4 签名密钥
   */
  private deriveSigningKey(dateStamp: string): number[] {
    const kDate = sha256.hmac
      .create('AWS4' + this.secretAccessKey)
      .update(dateStamp)
      .array();
    const kRegion = sha256.hmac.create(kDate).update(this.region).array();
    const kService = sha256.hmac.create(kRegion).update('s3').array();
    const kSigning = sha256.hmac.create(kService).update('aws4_request').array();
    return kSigning;
  }

  /**
   * 格式化 AMZ 日期：20260419T120000Z
   */
  private formatAmzDate(date: Date): string {
    return date.toISOString().replace(/[:-]|\.\d{3}/g, '');
  }

  /**
   * URI 编码 canonical path（保留 /）
   */
  private encodeCanonicalUri(path: string): string {
    if (!path || path === '/') return '/';
    const hasTrailingSlash = path.endsWith('/');
    const encoded =
      '/' +
      path
        .split('/')
        .filter((s) => s)
        .map((segment) => this.uriEncode(segment))
        .join('/');
    return hasTrailingSlash ? encoded + '/' : encoded;
  }

  /**
   * AWS URI 编码（RFC 3986，不编码 ~）
   */
  private uriEncode(str: string): string {
    return encodeURIComponent(str).replace(/%7E/gi, '~');
  }

  /**
   * 构建 canonical query string（参数按字典序排列）
   */
  private buildCanonicalQueryString(params: Record<string, string>): string {
    return Object.keys(params)
      .sort()
      .map((key) => `${this.uriEncode(key)}=${this.uriEncode(params[key])}`)
      .join('&');
  }

  /**
   * 构建 canonical headers 和 signed headers
   */
  private buildCanonicalHeaders(headers: Record<string, string>): {
    canonicalHeaders: string;
    signedHeaders: string;
  } {
    const sorted = Object.entries(headers)
      .map(([k, v]) => [k.toLowerCase().trim(), v.trim()] as const)
      .sort(([a], [b]) => a.localeCompare(b));

    const canonicalHeaders = sorted.map(([k, v]) => `${k}:${v}\n`).join('');
    const signedHeaders = sorted.map(([k]) => k).join(';');

    return { canonicalHeaders, signedHeaders };
  }

  // ─── 工具方法 ─────────────────────────────────────────────────

  /**
   * 验证配置
   */
  private validateConfig(config: S3ClientConfig): void {
    if (!config.bucketName?.trim()) {
      throw new ValidationError('Bucket name is required');
    }
    if (!config.accessKeyId?.trim()) {
      throw new ValidationError('Access Key ID is required');
    }
    if (!config.secretAccessKey) {
      throw new ValidationError('Secret Access Key is required');
    }
  }

  /**
   * 验证 ProfileDto
   */
  private validateProfile(profile: ProfileDto): void {
    if (!profile) throw new ValidationError('Profile is required');
    if (!profile.type) throw new ValidationError('Profile type is required');

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

  /**
   * 包装错误为统一格式
   */
  private wrapError(error: unknown): Error {
    if (error instanceof APIError) return error;

    if (axios.isAxiosError(error)) {
      if (error.code === 'ERR_CANCELED' || error.name === 'CanceledError') {
        const cancelError = new APIError('Request cancelled');
        cancelError.name = 'AbortError';
        return cancelError;
      }
      if (!error.response) {
        if (error.code === 'ECONNABORTED' || error.message.includes('timeout')) {
          return new TimeoutError('Request timeout');
        }
        return new NetworkError('Network request failed', error);
      }
      const { status, data } = error.response;
      if (status === 401 || status === 403) {
        const detail = typeof data === 'string' ? this.extractS3ErrorMessage(data) : '';
        return new ServerError(
          detail || 'S3 authentication failed: check Access Key and Secret',
          status,
          data
        );
      }
      if (status === 404) {
        return new ServerError('S3 object not found', status, data);
      }
      const msg = typeof data === 'string' ? this.extractS3ErrorMessage(data) : `HTTP ${status}`;
      return new ServerError(msg, status, data);
    }

    return error instanceof Error ? error : new Error(String(error));
  }

  /**
   * 从 S3 XML 错误响应中提取错误消息
   */
  private extractS3ErrorMessage(xml: string): string {
    const codeMatch = xml.match(/<Code>(.*?)<\/Code>/);
    const messageMatch = xml.match(/<Message>(.*?)<\/Message>/);
    if (codeMatch && messageMatch) {
      return `S3 Error: ${codeMatch[1]} - ${messageMatch[1]}`;
    }
    if (messageMatch) {
      return `S3 Error: ${messageMatch[1]}`;
    }
    return 'S3 request failed';
  }
}
