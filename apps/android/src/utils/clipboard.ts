/**
 * Clipboard Type Converter
 * 剪贴板内容与 API DTO 之间的类型转换
 */

import { ProfileDto, ClipboardContent, ClipboardContentType } from '@/types';
import { calculateContentHash } from '@/utils/hash';
import { isTextInvalid } from './textUtils';

export interface ContentToProfileDtoOptions {
  signal?: AbortSignal;
}

/**
 * 将 ClipboardContent 转换为 ProfileDto
 */
export async function contentToProfileDto(
  content: ClipboardContent,
  options?: ContentToProfileDtoOptions
): Promise<ProfileDto> {
  let { type, text = '', profileHash, fileName, fileSize, fileUri } = content;

  // 如果没有提供 profileHash，则计算
  if (!profileHash) {
    profileHash = await calculateContentHash(content, options?.signal);
  }

  switch (type) {
    case 'Text': {
      // 如果有 fileUri，说明文本已经被保存为文件（由 ClipboardManager 处理）
      if (fileUri && fileName) {
        return {
          type: 'Text',
          text, // 预览文本
          hash: profileHash,
          hasData: true, // 标记有外部文件
          dataName: fileName,
          size: fileSize,
        };
      }

      // 短文本直接返回
      return {
        type: 'Text',
        text,
        hash: profileHash,
        hasData: false,
      };
    }

    case 'Image':
      return {
        type: 'Image',
        text: text || '[图片]',
        hash: profileHash,
        hasData: true,
        dataName: fileName,
        size: fileSize,
      };

    case 'File':
      return {
        type: 'File',
        text: text || fileName || '[文件]',
        hash: profileHash,
        hasData: true,
        dataName: fileName,
        size: fileSize,
      };

    case 'Group':
      return {
        type: 'Group',
        text: text || '[文件组]',
        hash: profileHash,
        hasData: true,
        dataName: fileName,
        size: fileSize,
      };

    default:
      throw new Error(`Unsupported clipboard type: ${type}`);
  }
}

/**
 * 将 ProfileDto 转换为 ClipboardContent
 */
export function profileDtoToContent(profile: ProfileDto): ClipboardContent {
  const { type, text, hash, hasData, dataName, size } = profile;

  const baseContent: ClipboardContent = {
    type: type as ClipboardContentType,
    text,
    profileHash: hash,
    timestamp: Date.now(), // 添加当前时间戳
    hasData, // 添加 hasData 字段
  };

  if (hasData) {
    switch (type) {
      case 'Text':
        // Text类型的hasData为true时，需要下载额外的.txt文件获取完整文本
        return {
          ...baseContent,
          fileName: dataName,
          fileSize: size || text?.length || 0, // 如果 size 为 0，使用文本长度
        };

      case 'Image':
        return {
          ...baseContent,
          fileName: dataName,
          fileSize: size,
        };

      case 'File':
      case 'Group':
        return {
          ...baseContent,
          fileName: dataName,
          fileSize: size,
        };
    }
  }

  // hasData 为 false 时，Text 类型仍然需要 fileSize（字符数）
  if (type === 'Text') {
    return {
      ...baseContent,
      fileSize: size || text?.length || 0, // 使用 size 或文本长度
    };
  }

  return baseContent;
}

/**

 * 从 MIME 类型获取文件扩展名
 */
export function getExtensionFromMimeType(mimeType: string): string {
  const mimeToExt: Record<string, string> = {
    'image/jpeg': 'jpg',
    'image/jpg': 'jpg',
    'image/png': 'png',
    'image/gif': 'gif',
    'image/webp': 'webp',
    'image/svg+xml': 'svg',
    'image/bmp': 'bmp',
    'text/plain': 'txt',
    'text/html': 'html',
    'application/pdf': 'pdf',
    'application/zip': 'zip',
    'application/json': 'json',
    'application/xml': 'xml',
  };

  return mimeToExt[mimeType.toLowerCase()] || 'bin';
}

/**
 * 从文件名获取扩展名
 */
export function getExtensionFromFileName(fileName: string): string {
  const match = fileName.match(/\.([^.]+)$/);
  return match ? match[1].toLowerCase() : 'bin';
}

/**
 * 格式化文件大小
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';

  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
}

/**
 * 获取剪贴板类型的显示名称
 */
export function getClipboardTypeDisplayName(type: ClipboardContentType): string {
  const displayNames: Record<ClipboardContentType, string> = {
    Text: '文本',
    Image: '图片',
    File: '文件',
    Group: '文件组',
  };

  return displayNames[type] || '未知';
}

/**
 * 获取剪贴板类型的图标名称（可用于 UI 图标）
 */
export function getClipboardTypeIcon(type: ClipboardContentType): string {
  const icons: Record<ClipboardContentType, string> = {
    Text: 'text',
    Image: 'image',
    File: 'file',
    Group: 'folder',
  };

  return icons[type] || 'help';
}

/**
 * 截断文本预览
 */
export function truncateText(text: string, maxLength: number = 100): string {
  if (text.length <= maxLength) {
    return text;
  }
  return text.substring(0, maxLength) + '...';
}

/**
 * 验证剪贴板内容
 */
export function validateClipboardContent(content: ClipboardContent): boolean {
  if (!content || !content.type) {
    return false;
  }

  switch (content.type) {
    case 'Text':
      return typeof content.text === 'string' && content.text.length > 0;

    case 'Image':
      return Boolean(content.fileUri || content.fileName);

    case 'File':
    case 'Group':
      return Boolean(content.fileUri || content.fileName);

    default:
      return false;
  }
}

/**
 * 剪贴板项目复制结果
 */
export interface CopyResult {
  success: boolean;
  message: string;
}

/**
 * 复制剪贴板项目到系统剪贴板
 * @param item 剪贴板项目（可以是 ClipboardContent 或 ClipboardItem）
 * @param clipboardManager 剪贴板管理器实例
 * @returns 复制结果
 */
export async function copyClipboardItem(
  item: {
    type: string;
    text?: string;
    fileUri?: string;
    profileHash?: string;
  },
  clipboardManager: {
    setClipboardContent: (content: ClipboardContent) => Promise<void>;
    setImageContent: (uri: string) => Promise<void>;
  }
): Promise<CopyResult> {
  try {
    if (item.type === 'Text' && !isTextInvalid(item.text)) {
      await clipboardManager.setClipboardContent({
        type: 'Text',
        text: item.text,
        profileHash: item.profileHash,
      });
      return { success: true, message: '已复制到剪贴板' };
    }

    if (item.type === 'Image' && item.fileUri) {
      await clipboardManager.setImageContent(item.fileUri);
      return { success: true, message: '已复制图片到剪贴板' };
    }

    return { success: false, message: '暂不支持此类型的快速复制' };
  } catch (error) {
    console.error('[copyClipboardItem] Failed to copy:', error);

    // 提取错误信息
    let errorMessage = '复制失败';
    if (error instanceof Error) {
      // 将整个错误转为字符串进行检查（包括多层堆栈）
      const fullErrorString = error.toString() + ' ' + error.message;
      console.log('[copyClipboardItem] Full error string:', fullErrorString);

      if (fullErrorString.includes('TransactionTooLargeException')) {
        errorMessage = '文本内容过大，无法复制到剪贴板（超过系统限制）';
      } else if (fullErrorString.includes('setStringAsync')) {
        // 提取更简洁的错误信息
        errorMessage = '复制失败：' + (error.message || '未知错误');
      } else {
        errorMessage = error.message || '复制失败';
      }
    }

    return { success: false, message: errorMessage };
  }
}

/**
 * 将内容写入系统剪贴板。
 * 只负责复制操作，不更新 Store。
 * 调用者负责在成功后更新 UI 状态。
 */
export async function copyToLocalClipboard(content: ClipboardContent): Promise<CopyResult> {
  const { clipboardManager, clipboardMonitor } = await import('@/services');

  clipboardMonitor.pausePolling();
  try {
    let contentToCopy = content;
    if (content.type === 'Text' && content.fileUri && content.hasData) {
      try {
        const response = await fetch(content.fileUri);
        const completeText = await response.text();
        console.log(
          `[copyToLocalClipboard] Read complete text from file for profileHash: ${content.profileHash}, length: ${completeText.length}`
        );
        contentToCopy = {
          ...content,
          text: completeText,
        };
      } catch (error) {
        console.error('[copyToLocalClipboard] Failed to read text file:', error);
        if (isTextInvalid(content.text)) {
          return { success: false, message: '无法读取完整文本' };
        }
      }
    }

    const result = await copyClipboardItem(contentToCopy, clipboardManager);
    if (result.success) {
      await clipboardMonitor.setLastContent(contentToCopy);
    }
    return result;
  } catch (error) {
    console.error('[copyToLocalClipboard] Failed to copy:', error);
    return { success: false, message: '复制失败' };
  } finally {
    clipboardMonitor.resumePolling();
  }
}
