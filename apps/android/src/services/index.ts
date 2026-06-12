/**
 * Services Entry Point
 * Exports all API clients and services
 */

// Error classes
export * from './errors';

// Authentication
export { AuthService, type Credentials } from './AuthService';

// API Clients
export { APIClient, type APIClientConfig, type ISyncClipboardAPI } from './APIClient';
export { SyncClipboardClient } from './SyncClipboardClient';
export { WebDAVClient, type WebDAVConfig } from './WebDAVClient';
export { S3Client, type S3ClientConfig } from './S3Client';

// Clipboard Services
export { ClipboardManager, clipboardManager } from './ClipboardManager';
export { ClipboardMonitor, clipboardMonitor } from './ClipboardMonitor';

// SignalR Client (re-exported from signalr-client module)
export { getSignalRClient, resetSignalRClient } from 'signalr-client';
export type {
  SignalRClient,
  RemoteClipboardChangedCallback,
  RemoteHistoryChangedCallback,
} from 'signalr-client';

// Sync Manager
export { SyncManager } from './SyncManager';

// Remote Clipboard Sync Service
export { getClipboardSyncService as getClipboardSyncService } from './ClipboardSyncService';

// Shortcut Service
export { ShortcutService } from './ShortcutService';

// Update Service
export { checkForUpdate, parseVersion, compareVersions, versionToStr } from './UpdateService';
export type { UpdateCheckResult, ParsedVersion, ReleaseAssetInfo } from './UpdateService';

// APK Download Service
export {
  getPreferredAbi,
  findAssetForAbi,
  checkApkCache,
  downloadApk,
  installApk,
  getApkCachePath,
  cleanOldApkCache,
} from './ApkDownloadService';
export type { ApkDownloadOptions, ApkDownloadProgress, ApkSource } from './ApkDownloadService';

// Storage Services
export { ConfigStorage, configStorage } from './ConfigStorage';
export { HistoryStorage, historyStorage } from './HistoryStorage';
export { CacheManager, cacheManager } from './CacheManager';
export { SecureStorage, secureStorage } from './SecureStorage';

// Logger Service
export {
  initLogger,
  getLogger,
  setLogLevel,
  getLogDirectory,
  getLogFilePaths,
  getLogFileUris,
  calculateLogSize,
  clearLogs,
  cleanOldLogs,
  log,
  saveLogsToFile,
  type LogConfig,
  type LogLevel,
} from './Logger';

// Factory function to create appropriate API client
import { SyncClipboardClient } from './SyncClipboardClient';
import { WebDAVClient } from './WebDAVClient';
import { S3Client } from './S3Client';
import { AuthService } from './AuthService';
import { ServerConfig } from '../types/api';
import { ConfigurationError } from './errors';
import { ISyncClipboardAPI } from './APIClient';

/**
 * 创建 API 客户端工厂函数
 */
export function createAPIClient(config: ServerConfig): ISyncClipboardAPI {
  const { type, url, username, password } = config;

  if (type === 'syncclipboard') {
    if (!url) {
      throw new ConfigurationError('Server URL is required');
    }
    const authService = username && password ? new AuthService(username, password) : undefined;
    return new SyncClipboardClient({ baseURL: url, authService });
  }

  if (type === 's3') {
    if (!config.bucketName) {
      throw new ConfigurationError('Bucket name is required for S3');
    }
    if (!username || !password) {
      throw new ConfigurationError('Access Key ID and Secret Access Key are required for S3');
    }
    return new S3Client({
      serviceURL: url || undefined,
      region: config.region,
      bucketName: config.bucketName,
      objectPrefix: config.objectPrefix,
      forcePathStyle: config.forcePathStyle,
      accessKeyId: username,
      secretAccessKey: password,
    });
  }

  // 非 SyncClipboard/S3 服务器，使用 WebDAV 客户端
  if (!url) {
    throw new ConfigurationError('Server URL is required');
  }
  if (!username || !password) {
    throw new ConfigurationError('Username and password are required for WebDAV');
  }
  return new WebDAVClient({ baseURL: url, username, password });
}
