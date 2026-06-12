/**
 * ClipboardSyncService Store
 * 远程剪贴板状态（由 ClipboardSyncService 维护，UI 层只读）
 */

import { create } from 'zustand';
import { ClipboardContent } from '../types/clipboard';
import type { ProgressInfo } from 'native-util';

/** 下载进度信息 */
export interface DownloadProgressInfo {
  progress: number;
  bytesTransferred: number;
  totalBytes: number;
}

/** 文件上传进度信息 */
export interface FileUploadProgressInfo {
  /** 上传阶段文本（如"正在处理文件…"/"正在上传文件…"） */
  stage: string;
  /** 详细进度信息（含 bytesTransferred/totalBytes），null 表示未知进度 */
  progressInfo: ProgressInfo | null;
}

interface ClipboardSyncServiceState {
  /** 当前远程剪贴板内容 */
  remoteContent: ClipboardContent | null;
  /** 是否正在加载远程内容 */
  loadingRemote: boolean;
  /** 是否正在下载远程文件（WebDAV 直接下载或 SyncClipboard 队列下载均通过此字段反映） */
  downloadingRemote: boolean;
  /** 远程文件下载进度（null 表示未在下载或不支持进度） */
  downloadProgress: DownloadProgressInfo | null;
  /** 是否正在上传剪贴板 */
  uploadingClipboard: boolean;
  /** 文件上传进度（null 表示未在上传） */
  fileUploadProgress: FileUploadProgressInfo | null;

  // ─── 内部写入接口（仅供 ClipboardSyncService 调用）───────────────
  setRemoteContent: (content: ClipboardContent | null) => void;
  setLoadingRemote: (loading: boolean) => void;
  setDownloadingRemote: (downloading: boolean) => void;
  setDownloadProgress: (progress: DownloadProgressInfo | null) => void;
  setUploadingClipboard: (uploading: boolean) => void;
  setFileUploadProgress: (progress: FileUploadProgressInfo | null) => void;
}

export const useClipboardSyncServiceStore = create<ClipboardSyncServiceState>((set) => ({
  remoteContent: null,
  loadingRemote: false,
  downloadingRemote: false,
  downloadProgress: null,
  uploadingClipboard: false,
  fileUploadProgress: null,

  setRemoteContent: (content) => set({ remoteContent: content }),
  setLoadingRemote: (loading) => set({ loadingRemote: loading }),
  setDownloadingRemote: (downloading) => set({ downloadingRemote: downloading }),
  setDownloadProgress: (progress) => set({ downloadProgress: progress }),
  setUploadingClipboard: (uploading) => set({ uploadingClipboard: uploading }),
  setFileUploadProgress: (progress) => set({ fileUploadProgress: progress }),
}));
