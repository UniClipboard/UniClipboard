/**
 * uploadFileAndAddToHistory
 * 将本地文件（content:// 或 file:// URI）复制到 temp 目录、写入历史记录并上传到服务器。
 * 供 HomeScreen 右上角"上传文件"菜单和 ShareReceiveScreen 共同调用。
 */

import { File } from 'expo-file-system';
import { nativeCopyFile, type ProgressInfo } from 'native-util';
import { calculateFileProfileHash, calculateTextHash } from '@/utils/hash';
import { prepareTempFilePath } from '@/utils/fileStorage';
import { useHistoryStore } from '@/stores/historyStore';
import { createAPIClient } from '@/services';
import { SyncManager } from '@/services/SyncManager';
import type { ClipboardContent } from '@/types/clipboard';
import { createDefaultClipboardItem, HistorySyncStatus } from '@/types/clipboard';
import type { ClipboardContentType } from '@/types/api';
import type { ServerConfig } from '@/types/api';

function guessContentType(mimeType: string | null | undefined): ClipboardContentType {
  if (!mimeType) return 'File';
  if (mimeType.startsWith('image/')) return 'Image';
  return 'File';
}

export interface UploadFileOptions {
  signal?: AbortSignal;
  onProgress?: (stage: string, progress?: ProgressInfo) => void;
}

export interface ImportResult {
  profileHash: string;
  fileUri: string;
  fileName: string;
  fileSize: number;
  contentType: ClipboardContentType;
}

export async function importFileToHistory(
  sourceUri: string,
  fileName: string,
  mimeType: string | null | undefined,
  fileSize: number | undefined,
  options?: UploadFileOptions
): Promise<ImportResult> {
  const contentType: ClipboardContentType = guessContentType(mimeType);
  const tempPath = prepareTempFilePath(fileName);
  const sourceFile = new File(sourceUri);
  options?.onProgress?.('正在复制文件…');
  await nativeCopyFile(sourceFile.uri, tempPath);

  options?.onProgress?.('正在计算哈希…');
  const profileHash = await calculateFileProfileHash(tempPath, fileName);
  const resolvedSize = fileSize ?? sourceFile.size;

  const savedItem = await useHistoryStore.getState().addItem(
    createDefaultClipboardItem({
      type: contentType,
      text: fileName,
      profileHash,
      hasData: true,
      dataName: fileName,
      size: resolvedSize,
      timestamp: Date.now(),
      fileUri: tempPath,
    })
  );

  return {
    profileHash,
    fileUri: savedItem.fileUri ?? tempPath,
    fileName,
    fileSize: resolvedSize,
    contentType,
  };
}

export async function uploadTextAndAddToHistory(
  text: string,
  activeServer: ServerConfig,
  options?: { signal?: AbortSignal }
): Promise<void> {
  const profileHash = await calculateTextHash(text, options?.signal);

  // 预先设置 hash，避免 SignalR/轮询推送时误判为新远程内容触发自动下载
  SyncManager.getInstance().setLastUploadedHash(profileHash);

  const content: ClipboardContent = {
    type: 'Text',
    text,
    profileHash,
    localClipboardHash: profileHash,
    hasData: false,
    timestamp: Date.now(),
  };

  const apiClient = createAPIClient(activeServer);
  await apiClient.putContent(content, { signal: options?.signal });

  const historyItem = createDefaultClipboardItem({
    type: 'Text',
    text,
    profileHash,
    hasData: false,
    timestamp: Date.now(),
    syncStatus: HistorySyncStatus.Synced,
  });
  await useHistoryStore.getState().addItem(historyItem);
}

export async function uploadFileAndAddToHistory(
  sourceUri: string,
  fileName: string,
  mimeType: string | null | undefined,
  fileSize: number | undefined,
  activeServer: ServerConfig,
  options?: UploadFileOptions
): Promise<void> {
  const result = await importFileToHistory(sourceUri, fileName, mimeType, fileSize, options);

  // 预先设置 hash，避免 SignalR/轮询推送时误判为新远程内容触发自动下载
  SyncManager.getInstance().setLastUploadedHash(result.profileHash);

  const content: ClipboardContent = {
    type: result.contentType,
    text: result.fileName,
    fileUri: result.fileUri,
    fileName: result.fileName,
    fileSize: result.fileSize,
    profileHash: result.profileHash,
    localClipboardHash: result.profileHash,
    hasData: true,
    timestamp: Date.now(),
  };

  const apiClient = createAPIClient(activeServer);
  options?.onProgress?.('正在上传文件…');
  await apiClient.putContent(content, {
    signal: options?.signal,
    onProgress: (info) => options?.onProgress?.('正在上传文件…', info),
  });

  await useHistoryStore.getState().updateItem(result.profileHash, { synced: true });
}
