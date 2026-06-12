/**
 * API Related Types
 */

/**
 * 剪贴板类型枚举
 */
export type ClipboardContentType = 'Text' | 'Image' | 'File' | 'Group';

/**
 * 剪贴板配置 DTO
 * 与服务器端 ProfileDto 对应
 */
export interface ProfileDto {
  /** 剪贴板内容类型 */
  type: ClipboardContentType;

  /** Profile SHA256 哈希值（遵循服务器规则，用于去重和验证） */
  hash?: string;

  /** 预览文本或完整文本内容 */
  text: string;

  /** 是否有额外的数据文件（如图片、文件等） */
  hasData: boolean;

  /** 数据文件名 */
  dataName?: string;

  /** 文件大小（字节） */
  size?: number;
}

/**
 * 服务器配置
 */
export interface ServerConfig {
  /** 服务器类型 */
  type: 'syncclipboard' | 'webdav' | 's3';

  /** 服务器显示名称（可选） */
  name?: string;

  /** 服务器 URL */
  url: string;

  /** 用户名（S3 时为 Access Key ID） */
  username?: string;

  /** 密码（S3 时为 Secret Access Key） */
  password?: string;

  /** S3 区域（仅 S3 类型） */
  region?: string;

  /** S3 存储桶名称（仅 S3 类型） */
  bucketName?: string;

  /** S3 对象 key 前缀（仅 S3 类型） */
  objectPrefix?: string;

  /** S3 是否使用路径风格寻址（仅 S3 类型） */
  forcePathStyle?: boolean;
}

/**
 * 同步操作类型
 */
export type SyncOperation = 'upload' | 'download' | 'bidirectional';

/**
 * API 同步结果
 */
export interface APISyncResult {
  /** 是否成功 */
  success: boolean;

  /** 操作类型 */
  operation: SyncOperation;

  /** 同步的内容 */
  profile?: ProfileDto;

  /** 错误信息 */
  error?: string;

  /** 同步时间戳 */
  timestamp: number;
}

/**
 * 服务器信息
 */
export interface ServerInfo {
  /** 服务器版本 */
  version: string;

  /** 服务器时间 */
  serverTime: Date;

  /** 是否在线 */
  online: boolean;
}
