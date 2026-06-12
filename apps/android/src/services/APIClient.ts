/**
 * API Client Base Class
 * Handles HTTP requests with authentication and error handling
 */

import axios, { AxiosInstance, AxiosRequestConfig, AxiosError } from 'axios';
import { AuthService } from './AuthService';
import { APP_NAME, APP_VERSION } from '../constants';
import { ProfileDto, ServerInfo } from '../types/api';
import type { ClipboardContent } from '../types/clipboard';
import { nativeDownloadFile, type ProgressInfo } from 'native-util';
import {
  APIError,
  AuthenticationError,
  NetworkError,
  ServerError,
  TimeoutError,
  ConfigurationError,
} from './errors';

/**
 * 扩展的错误接口，包含网络错误标志和原始错误
 */
interface ExtendedError extends Error {
  isNetworkError?: boolean;
  originalError?: unknown;
}

/**
 * API 客户端配置
 */
export interface APIClientConfig {
  /** 基础 URL */
  baseURL: string;

  /** 超时时间（毫秒） */
  timeout?: number;

  /** 认证服务 */
  authService?: AuthService;

  /** 附加请求头 */
  headers?: Record<string, string>;
}

export interface PutContentOptions {
  signal?: AbortSignal;
  onProgress?: (info: ProgressInfo) => void;
}

/**
 * SyncClipboard API 接口（基础功能）
 */
export interface DownloadProgressCallback {
  (info: ProgressInfo): void;
}

export interface ISyncClipboardAPI {
  /** 获取剪贴板配置 */
  getClipboard(signal?: AbortSignal): Promise<ProfileDto>;

  /** 上传剪贴板配置 */
  putClipboard(profile: ProfileDto, signal?: AbortSignal): Promise<void>;

  /** 直接下载文件到指定路径（优化内存占用） */
  downloadFile(
    fileName: string,
    destinationUri: string,
    signal?: AbortSignal,
    onProgress?: DownloadProgressCallback
  ): Promise<string>;

  /** 上传文件数据 */
  putFile(
    fileName: string,
    fileUri: string,
    signal?: AbortSignal,
    onProgress?: DownloadProgressCallback
  ): Promise<void>;

  /**
   * 上传剪贴板内容
   * 先上传数据文件（如果有），再上传配置
   */
  putContent(content: ClipboardContent, options?: PutContentOptions): Promise<void>;

  /** 获取服务器时间 */
  getServerTime(signal?: AbortSignal): Promise<Date>;

  /** 获取服务器版本 */
  getVersion(): Promise<string>;

  /** 获取服务器信息 */
  getServerInfo(): Promise<ServerInfo>;

  /** 测试连接 */
  testConnection(signal?: AbortSignal): Promise<void>;
}

/**
 * API 客户端基类
 */
export abstract class APIClient {
  protected client: AxiosInstance;
  protected authService?: AuthService;
  protected baseURL: string;

  constructor(config: APIClientConfig) {
    const { baseURL, timeout = 30000, authService, headers = {} } = config;

    if (!baseURL) {
      throw new ConfigurationError('Base URL is required');
    }

    this.baseURL = baseURL.replace(/\/+$/, '');
    this.authService = authService;

    // 创建 Axios 实例
    this.client = axios.create({
      baseURL: this.baseURL,
      timeout,
      headers: {
        'Content-Type': 'application/json',
        'User-Agent': `${APP_NAME}/${APP_VERSION}`,
        ...headers,
      },
    });

    // 设置拦截器
    this.setupInterceptors();
  }

  /**
   * 设置请求和响应拦截器
   */
  private setupInterceptors(): void {
    // 请求拦截器 - 添加认证头
    this.client.interceptors.request.use(
      (config) => {
        // 添加认证头
        if (this.authService?.isConfigured()) {
          try {
            config.headers.Authorization = this.authService.getAuthHeader();
          } catch (error) {
            console.warn('Failed to add auth header:', error);
          }
        }

        // 日志：请求信息
        console.log(`[API] ${config.method?.toUpperCase()} ${config.url}`);

        return config;
      },
      (error) => {
        return Promise.reject(this.handleError(error));
      }
    );

    // 响应拦截器 - 统一错误处理
    this.client.interceptors.response.use(
      (response) => {
        // 日志：响应信息
        console.log(`[API] Response ${response.status} ${response.config.url}`);

        return response;
      },
      (error) => {
        return Promise.reject(this.handleError(error));
      }
    );
  }

  /**
   * 统一错误处理
   */
  protected handleError(error: unknown): APIError {
    if (axios.isAxiosError(error)) {
      const axiosError = error as AxiosError;

      // 检查是否是取消请求的错误
      if (
        axiosError.code === 'ERR_CANCELED' ||
        axiosError.message.includes('cancel') ||
        error.name === 'AbortError' ||
        axiosError.name === 'CanceledError'
      ) {
        const cancelError = new APIError('Request cancelled');
        cancelError.name = 'AbortError';
        return cancelError;
      }

      // 网络错误
      if (!axiosError.response) {
        if (axiosError.code === 'ECONNABORTED' || axiosError.message.includes('timeout')) {
          return new TimeoutError('Request timeout');
        }
        return new NetworkError('Network request failed', axiosError);
      }

      // HTTP 错误
      const { status, data } = axiosError.response;

      // 记录响应详情
      console.error('[APIClient] HTTP Error - Status:', status);
      console.error('[APIClient] Response data:', JSON.stringify(data, null, 2));

      // 401 未授权
      if (status === 401) {
        return new AuthenticationError('Invalid credentials or authentication failed');
      }

      // 403 禁止访问
      if (status === 403) {
        return new AuthenticationError('Access forbidden');
      }

      // 404 未找到
      if (status === 404) {
        return new ServerError('Resource not found', status, data);
      }

      // 500+ 服务器错误
      if (status >= 500) {
        const responseMsg = this.extractResponseMessage(data);
        const message = responseMsg
          ? `Server error (HTTP ${status}): ${responseMsg}`
          : `Server error (HTTP ${status})`;
        return new ServerError(message, status, data);
      }

      // 其他 HTTP 错误（如 400）
      const responseMsg = this.extractResponseMessage(data);
      const message = responseMsg
        ? `HTTP ${status}: ${responseMsg}`
        : `HTTP ${status}: ${axiosError.message}`;
      return new ServerError(message, status, data);
    }

    // 其他类型的错误
    if (error instanceof APIError) {
      return error;
    }

    // 未知错误
    const message = error instanceof Error ? error.message : 'Unknown error occurred';
    return new APIError(message);
  }

  /**
   * 从响应数据中提取错误消息
   */
  private extractResponseMessage(data: unknown): string | null {
    if (!data) return null;

    // 如果是字符串，直接返回
    if (typeof data === 'string') {
      return data;
    }

    // 如果是对象，尝试提取常见字段
    if (typeof data === 'object') {
      const obj = data as Record<string, unknown>;

      // 尝试常见错误消息字段
      if (obj.message && typeof obj.message === 'string') {
        return obj.message;
      }
      if (obj.error && typeof obj.error === 'string') {
        return obj.error;
      }
      if (obj.errorMessage && typeof obj.errorMessage === 'string') {
        return obj.errorMessage;
      }
      if (obj.detail && typeof obj.detail === 'string') {
        return obj.detail;
      }
      if (obj.title && typeof obj.title === 'string') {
        return obj.title;
      }

      // 如果有嵌套的 error 对象
      if (obj.error && typeof obj.error === 'object') {
        const errorObj = obj.error as Record<string, unknown>;
        if (errorObj.message && typeof errorObj.message === 'string') {
          return errorObj.message;
        }
      }
    }

    return null;
  }

  /**
   * 设置认证服务
   */
  setAuthService(authService: AuthService): void {
    this.authService = authService;
  }

  /**
   * 获取基础 URL
   */
  getBaseURL(): string {
    return this.baseURL;
  }

  /**
   * 获取包含认证信息的请求头
   */
  protected async getHeaders(): Promise<Record<string, string>> {
    const headers: Record<string, string> = {
      'User-Agent': `${APP_NAME}/${APP_VERSION}`,
    };

    // 添加认证头
    if (this.authService?.isConfigured()) {
      try {
        headers.Authorization = this.authService.getAuthHeader();
      } catch (error) {
        console.warn('Failed to add auth header:', error);
      }
    }

    return headers;
  }

  /**
   * 更新基础 URL
   */
  setBaseURL(baseURL: string): void {
    this.baseURL = baseURL.replace(/\/+$/, '');
    this.client.defaults.baseURL = this.baseURL;
  }

  /**
   * GET 请求
   */
  protected async get<T = unknown>(url: string, config?: AxiosRequestConfig): Promise<T> {
    const response = await this.client.get<T>(url, config);
    return response.data;
  }

  /**
   * POST 请求
   */
  protected async post<T = unknown>(
    url: string,
    data?: unknown,
    config?: AxiosRequestConfig
  ): Promise<T> {
    const response = await this.client.post<T>(url, data, config);
    return response.data;
  }

  /**
   * PUT 请求
   */
  protected async put<T = unknown>(
    url: string,
    data?: unknown,
    config?: AxiosRequestConfig
  ): Promise<T> {
    const response = await this.client.put<T>(url, data, config);
    return response.data;
  }

  /**
   * DELETE 请求
   */
  protected async delete<T = unknown>(url: string, config?: AxiosRequestConfig): Promise<T> {
    const response = await this.client.delete<T>(url, config);
    return response.data;
  }

  /**
   * PATCH 请求
   */
  protected async patch<T = unknown>(
    url: string,
    data?: unknown,
    config?: AxiosRequestConfig
  ): Promise<T> {
    const response = await this.client.patch<T>(url, data, config);
    return response.data;
  }

  private static readonly FILE_ENDPOINT = '/file/';

  /**
   * 直接下载文件到指定路径（优化内存占用）
   * 使用原生 HttpURLConnection 流式下载，每次仅持有 8KB 缓冲，不将文件内容读入 JVM/JS 堆内存
   */
  async downloadFile(
    fileName: string,
    destinationUri: string,
    signal?: AbortSignal,
    onProgress?: DownloadProgressCallback
  ): Promise<string> {
    if (!fileName) {
      throw new ConfigurationError('File name is required');
    }
    if (!destinationUri) {
      throw new ConfigurationError('Destination URI is required');
    }

    try {
      const url = `${this.baseURL}${APIClient.FILE_ENDPOINT}${encodeURIComponent(fileName)}`;
      const headers = await this.getHeaders();

      console.log(`[${this.constructor.name}] Downloading file ${fileName} to ${destinationUri}`);

      await nativeDownloadFile(url, headers, destinationUri, signal, onProgress);

      console.log(`[${this.constructor.name}] File downloaded successfully: ${fileName}`);
      return destinationUri;
    } catch (error) {
      console.error(`[${this.constructor.name}] Failed to download file ${fileName}:`, error);
      throw error;
    }
  }

  /**
   * 测试连接
   */
  async testConnection(signal?: AbortSignal): Promise<void> {
    await this.get('/', { signal });
  }

  /**
   * 上传文件数据（由子类实现）
   * @param fileName 服务器上的文件名
   * @param fileUri 本地文件的 URI
   */
  abstract putFile(fileName: string, fileUri: string, signal?: AbortSignal): Promise<void>;

  /**
   * 上传剪贴板配置（由子类实现）
   * @param profile 剪贴板配置对象
   */
  abstract putClipboard(profile: ProfileDto, signal?: AbortSignal): Promise<void>;

  /**
   * 构建详细的错误信息，包含HTTP状态码和服务器响应体
   */
  protected buildError(error: unknown, context: string): Error {
    console.error(context, ':', error);

    let errorMessage = error instanceof Error ? error.message : 'Unknown error';
    let isNetworkError = false;

    // 检查错误对象中是否有 statusCode 和 response 信息
    const hasStatusCode = error && typeof error === 'object' && 'statusCode' in error;
    const hasResponse = error && typeof error === 'object' && 'response' in error;

    if (hasStatusCode) {
      const errorObj = error as Record<string, unknown>;
      const statusCode = errorObj.statusCode;
      console.error(`${context} - Status code:`, statusCode);

      if (hasResponse) {
        const response = errorObj.response;
        console.error(`${context} - Server response:`, JSON.stringify(response, null, 2));

        // 构建包含状态码和响应体的完整错误信息
        const responseText =
          typeof response === 'string' ? response : JSON.stringify(response, null, 2);

        errorMessage = `服务器返回错误 (HTTP ${statusCode}):\n\n${responseText}`;
      } else {
        errorMessage = `服务器返回错误 (HTTP ${statusCode}): ${errorMessage}`;
      }
    } else if (hasResponse) {
      // 有response但没有statusCode（可能是Axios原始错误）
      const errorObj = error as Record<string, unknown>;
      const response = errorObj.response as Record<string, unknown> | undefined;
      console.error(`${context} - Server response:`, JSON.stringify(response, null, 2));

      if (response?.data) {
        const responseText =
          typeof response.data === 'string'
            ? response.data
            : JSON.stringify(response.data, null, 2);
        const status = response.status as number | undefined;
        errorMessage = `服务器返回错误 (HTTP ${status || 'unknown'}):\n\n${responseText}`;
      }
    }

    // 检查是否是网络错误
    if (error instanceof Error) {
      const message = error.message.toLowerCase();
      isNetworkError =
        message.includes('network') ||
        message.includes('timeout') ||
        message.includes('connection') ||
        message.includes('econnrefused') ||
        message.includes('offline');
    }

    // 创建新的错误对象，附加专有属性
    const builtError: ExtendedError = new Error(errorMessage);
    builtError.isNetworkError = isNetworkError;
    builtError.originalError = error;

    return builtError;
  }
}
