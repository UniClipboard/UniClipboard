/**
 * File Storage Utility
 * 文件存储工具 - 管理剪贴板文件的本地存储
 * 使用 Expo File System 新 API (File 和 Directory 类)
 */

import { Paths, File, Directory } from 'expo-file-system';

/**
 * 文件存储目录结构
 * clipboards/
 *   images/     - 图片文件
 *   files/      - 普通文件
 * history/      - 历史记录文件
 *   {Type}-{profileHash}/  - 按类型和profileHash组织的历史文件
 */
const BASE_DIR = new Directory(Paths.document, 'clipboards');
const IMAGE_DIR = new Directory(BASE_DIR, 'images');
const FILE_DIR = new Directory(BASE_DIR, 'files');
export const HISTORY_BASE_DIR = new Directory(BASE_DIR, 'history');

// 临时剪贴板文件存储目录（供 ClipboardManager 和 SyncManager 共用）
export const CLIPBOARD_TEMP_DIR = new Directory(Paths.cache, 'temp_files');

/**
 * 初始化文件存储目录
 */
export async function initFileStorage(): Promise<void> {
  try {
    // 使用新的 Directory API 创建目录（如果不存在）
    if (!BASE_DIR.exists) {
      BASE_DIR.create();
    }
    if (!IMAGE_DIR.exists) {
      IMAGE_DIR.create();
    }
    if (!FILE_DIR.exists) {
      FILE_DIR.create();
    }
    if (!HISTORY_BASE_DIR.exists) {
      HISTORY_BASE_DIR.create();
    }

    // console.log('[FileStorage] Initialized directories:', {
    //   base: BASE_DIR.uri,
    //   images: IMAGE_DIR.uri,
    //   files: FILE_DIR.uri,
    //   history: HISTORY_BASE_DIR.uri,
    // });
  } catch (error) {
    console.error('[FileStorage] Failed to initialize directories:', error);
    throw error;
  }
}

/**
 * 获取历史记录文件目录
 * @param type 文件类型
 * @param profileHash profileHash值
 * @returns 目录对象
 */
export function getHistoryFileDir(type: string, profileHash: string): Directory {
  return new Directory(HISTORY_BASE_DIR, `${type}-${profileHash}`);
}

/**
 * 保存历史记录文件
 * @param type 文件类型
 * @param profileHash profileHash值
 * @param fileName 文件名（使用dto中的dateName）
 * @param data 文件数据
 * @returns 文件URI
 */
export async function saveHistoryFile(
  type: string,
  profileHash: string,
  fileName: string,
  data: ArrayBuffer
): Promise<string> {
  try {
    // 确保基础目录存在
    await initFileStorage();

    // 获取历史文件目录
    const historyDir = getHistoryFileDir(type, profileHash);

    // 创建历史文件目录（如果不存在）
    if (!historyDir.exists) {
      historyDir.create();
    }

    // 创建文件
    const file = new File(historyDir, fileName);

    // 检查文件是否已存在
    if (file.exists) {
      console.log('[FileStorage] History file already exists:', file.uri);
      return file.uri;
    }

    // 将 ArrayBuffer 转换为 Uint8Array
    const uint8Array = new Uint8Array(data);

    // 写入文件
    file.write(uint8Array);

    console.log('[FileStorage] History file saved:', file.uri);
    return file.uri;
  } catch (error) {
    console.error('[FileStorage] Failed to save history file:', error);
    throw error;
  }
}

/**
 * 获取历史记录文件URI
 * @param type 文件类型
 * @param profileHash profileHash值
 * @param fileName 文件名
 * @returns 文件URI，如果文件不存在返回 null
 */
export async function getHistoryFileUri(
  type: string,
  profileHash: string,
  fileName: string
): Promise<string | null> {
  try {
    // 确保基础目录存在
    await initFileStorage();

    const historyDir = getHistoryFileDir(type, profileHash);
    const file = new File(historyDir, fileName);

    return file.exists ? file.uri : null;
  } catch (error) {
    console.error('[FileStorage] Failed to get history file URI:', error);
    return null;
  }
}

/**
 * 准备临时文件路径（确保目录存在并返回目标 URI）
 * @param fileName 文件名
 * @returns 目标文件URI
 */
export function prepareTempFilePath(fileName: string): string {
  if (!CLIPBOARD_TEMP_DIR.exists) {
    CLIPBOARD_TEMP_DIR.create();
  }
  return new File(CLIPBOARD_TEMP_DIR, fileName).uri;
}

/**
 * 准备历史记录文件的目标URI（创建目录但不要求文件已存在）
 * 用于 downloadFile 调用，传入目标路径后直接由下载接口写入文件
 * @param type 文件类型
 * @param profileHash profileHash值
 * @param fileName 文件名
 * @returns 目标文件URI
 */
export async function prepareHistoryFileUri(
  type: string,
  profileHash: string,
  fileName: string
): Promise<string> {
  // 确保基础目录存在
  await initFileStorage();

  const historyDir = getHistoryFileDir(type, profileHash);
  if (!historyDir.exists) {
    historyDir.create();
  }

  return new File(historyDir, fileName).uri;
}

/**
 * 删除历史记录文件目录
 * @param type 文件类型
 * @param profileHash profileHash值
 */
export async function deleteHistoryFileDir(type: string, profileHash: string): Promise<void> {
  try {
    const historyDir = getHistoryFileDir(type, profileHash);

    if (historyDir.exists) {
      historyDir.delete();
      console.log('[FileStorage] History file directory deleted:', historyDir.uri);
    }
  } catch (error) {
    console.error('[FileStorage] Failed to delete history file directory:', error);
    throw error;
  }
}

/**
 * 保存文件到本地存储
 * @param type 文件类型（Image 或 File）
 * @param fileHash 文件 hash 值
 * @param data 文件数据（ArrayBuffer）
 * @param extension 文件扩展名（可选，如 .jpg, .png, .pdf）
 * @returns 文件URI
 */
export async function saveFile(
  type: 'Image' | 'File',
  fileHash: string,
  data: ArrayBuffer,
  extension?: string
): Promise<string> {
  try {
    // 确保目录存在
    await initFileStorage();

    // 确定保存目录
    const dir = type === 'Image' ? IMAGE_DIR : FILE_DIR;

    // 生成文件名：使用hash值，保留扩展名
    const fileName = extension ? `${fileHash}${extension}` : fileHash;

    // 使用新的 File API
    const file = new File(dir, fileName);

    // 检查文件是否已存在
    if (file.exists) {
      console.log('[FileStorage] File already exists:', file.uri);
      return file.uri;
    }

    // 将 ArrayBuffer 转换为 Uint8Array
    const uint8Array = new Uint8Array(data);

    // 写入文件
    file.write(uint8Array);

    console.log('[FileStorage] File saved:', file.uri);
    return file.uri;
  } catch (error) {
    console.error('[FileStorage] Failed to save file:', error);
    throw error;
  }
}

/**
 * 根据 type 和 hash 获取文件URI
 * @param type 文件类型
 * @param fileHash 文件 hash 值
 * @param extension 文件扩展名（可选）
 * @returns 文件URI，如果文件不存在返回 null
 */
export async function getFileUri(
  type: 'Image' | 'File',
  fileHash: string,
  extension?: string
): Promise<string | null> {
  try {
    const dir = type === 'Image' ? IMAGE_DIR : FILE_DIR;
    const fileName = extension ? `${fileHash}${extension}` : fileHash;

    // 使用新的 File API 检查文件是否存在
    const file = new File(dir, fileName);

    return file.exists ? file.uri : null;
  } catch (error) {
    console.error('[FileStorage] Failed to get file URI:', error);
    return null;
  }
}

/**
 * 删除文件
 * @param type 文件类型
 * @param fileHash 文件 hash 值
 * @param extension 文件扩展名（可选）
 */
export async function deleteFile(
  type: 'Image' | 'File',
  fileHash: string,
  extension?: string
): Promise<void> {
  try {
    const dir = type === 'Image' ? IMAGE_DIR : FILE_DIR;
    const fileName = extension ? `${fileHash}${extension}` : fileHash;

    // 使用新的 File API
    const file = new File(dir, fileName);

    if (file.exists) {
      file.delete();
      console.log('[FileStorage] File deleted:', file.uri);
    }
  } catch (error) {
    console.error('[FileStorage] Failed to delete file:', error);
    throw error;
  }
}

/**
 * 清理所有剪贴板文件
 */
export async function clearAllFiles(): Promise<void> {
  try {
    // 使用新的 Directory API
    if (BASE_DIR.exists) {
      BASE_DIR.delete();
      console.log('[FileStorage] All files cleared');
    }
  } catch (error) {
    console.error('[FileStorage] Failed to clear files:', error);
    throw error;
  }
}

/**
 * 获取存储统计信息
 */
export async function getStorageStats(): Promise<{
  imageCount: number;
  fileCount: number;
  totalSize: number;
}> {
  try {
    let imageCount = 0;
    let fileCount = 0;
    let totalSize = 0;

    // 统计图片目录
    try {
      if (IMAGE_DIR.exists) {
        const images = IMAGE_DIR.list();
        imageCount = images.length;

        for (const imageName of images) {
          try {
            const imageFile = new File(IMAGE_DIR, imageName);
            if (imageFile.exists) {
              const info = imageFile.info();
              totalSize += info.size || 0;
            }
          } catch {
            // 忽略单个文件错误
          }
        }
      }
    } catch {
      // 目录不存在或其他错误
    }

    // 统计文件目录
    try {
      if (FILE_DIR.exists) {
        const files = FILE_DIR.list();
        fileCount = files.length;

        for (const fileName of files) {
          try {
            const file = new File(FILE_DIR, fileName);
            if (file.exists) {
              const info = file.info();
              totalSize += info.size || 0;
            }
          } catch {
            // 忽略单个文件错误
          }
        }
      }
    } catch {
      // 目录不存在或其他错误
    }

    return {
      imageCount,
      fileCount,
      totalSize,
    };
  } catch (error) {
    console.error('[FileStorage] Failed to get storage stats:', error);
    return {
      imageCount: 0,
      fileCount: 0,
      totalSize: 0,
    };
  }
}

/**
 * 直接下载文件并保存到本地（优化内存占用）
 * @param type 文件类型（Image 或 File）
 * @param fileHash 文件 hash 值
 * @param downloadUrl 下载URL
 * @param headers 请求头（用于认证等）
 * @param extension 文件扩展名（可选，如 .jpg, .png, .pdf）
 * @returns 文件URI
 */
export async function downloadAndSaveFile(
  type: 'Image' | 'File',
  fileHash: string,
  downloadUrl: string,
  headers?: Record<string, string>,
  extension?: string
): Promise<string> {
  try {
    // 确保目录存在
    await initFileStorage();

    // 确定保存目录
    const dir = type === 'Image' ? IMAGE_DIR : FILE_DIR;

    // 生成文件名：使用 fileHash 值，保留扩展名
    const fileName = extension ? `${fileHash}${extension}` : fileHash;

    // 使用新的 File API 检查文件是否已存在
    const file = new File(dir, fileName);

    if (file.exists) {
      console.log('[FileStorage] File already exists:', file.uri);
      return file.uri;
    }

    // 直接下载到文件系统（不占用内存）
    console.log('[FileStorage] Downloading file to:', file.uri);
    await File.downloadFileAsync(downloadUrl, file, {
      headers: headers || {},
    });

    console.log('[FileStorage] File downloaded successfully:', file.uri);
    return file.uri;
  } catch (error) {
    console.error('[FileStorage] Failed to download and save file:', error);
    throw error;
  }
}

/**
 * 从文件名中提取扩展名
 */
export function getFileExtension(fileName: string): string {
  const match = fileName.match(/\.[^.]+$/);
  return match ? match[0] : '';
}

/**
 * 计算目录大小
 * @param directory 目录对象
 * @returns 目录大小（字节）
 */
export function calculateDirectorySize(directory: Directory): number {
  try {
    let totalSize = 0;

    if (directory.exists) {
      const entries = directory.list();

      for (const entry of entries) {
        try {
          if (entry instanceof File) {
            // 处理文件
            const info = entry.info();
            totalSize += info.size || 0;
          } else if (entry instanceof Directory) {
            // 处理目录
            totalSize += calculateDirectorySize(entry);
          }
        } catch {
          // 忽略单个文件/目录错误
        }
      }
    }

    return totalSize;
  } catch (error) {
    console.error('[FileStorage] Failed to calculate directory size:', error);
    return 0;
  }
}

/**
 * 清空目录
 * @param directory 目录对象
 */
export function clearDirectory(directory: Directory): void {
  try {
    if (directory.exists) {
      const entries = directory.list();
      for (const entry of entries) {
        try {
          if (entry instanceof File) {
            // 处理文件
            entry.delete();
          } else if (entry instanceof Directory) {
            // 处理目录
            entry.delete();
          }
        } catch {
          // 忽略单个文件/目录错误
        }
      }
    }
  } catch (error) {
    console.error('[FileStorage] Failed to clear directory:', error);
    throw error;
  }
}
