/**
 * Tests for processRemoteContent
 *
 * 测试远程剪贴板内容解析逻辑，特别是上传图片后首页显示状态的正确性。
 * Bug: 上传图片成功后，fetchRemoteClipboard 拿到的 content 没有 fileUri，
 * 而 isJustUploaded 分支直接返回 content，未查询历史记录获取本地文件路径，
 * 导致首页远程卡片显示"未下载"。
 */

import { resolveRemoteContent, type ProcessRemoteContentDeps } from '../utils/processRemoteContent';
import type { ClipboardContent } from '../types/clipboard';

describe('resolveRemoteContent', () => {
  let deps: ProcessRemoteContentDeps;

  beforeEach(() => {
    deps = {
      getLastUploadedHash: jest.fn().mockReturnValue(null),
      getHistoryItem: jest.fn().mockResolvedValue(null),
      getHistoryFileUri: jest.fn().mockResolvedValue(null),
    };
  });

  describe('当内容未变化时', () => {
    it('历史记录无文件时应返回 null', async () => {
      const content: ClipboardContent = {
        type: 'Image',
        text: 'photo.png',
        profileHash: 'AABBCC',
        fileName: 'photo.png',
        fileSize: 5000,
        hasData: true,
      };

      const result = await resolveRemoteContent(content, 'AABBCC', 'AABBCC', true, deps);

      expect(result).toBeNull();
    });

    it('历史记录有文件时应返回 fileUriOnlyUpdate', async () => {
      const content: ClipboardContent = {
        type: 'Image',
        text: 'photo.png',
        profileHash: 'AABBCC',
        fileName: 'photo.png',
        fileSize: 5000,
        hasData: true,
      };

      (deps.getHistoryFileUri as jest.Mock).mockResolvedValue(
        'file:///data/history/Image/AABBCC/photo.png'
      );

      const result = await resolveRemoteContent(content, 'AABBCC', 'AABBCC', true, deps);

      expect(result).not.toBeNull();
      expect(result!.fileUriOnlyUpdate).toBe(true);
      expect(result!.foundInHistory).toBe(true);
      expect(result!.content.fileUri).toBe('file:///data/history/Image/AABBCC/photo.png');
    });
  });

  describe('当内容不是刚上传的，且历史记录有本地文件时', () => {
    it('应返回包含 fileUri 的 content', async () => {
      const content: ClipboardContent = {
        type: 'Image',
        text: 'photo.png',
        profileHash: 'AABBCC',
        fileName: 'photo.png',
        fileSize: 5000,
        hasData: true,
      };

      (deps.getHistoryItem as jest.Mock).mockResolvedValue({
        fileUri: 'file:///data/history/Image/AABBCC/photo.png',
      });
      (deps.getHistoryFileUri as jest.Mock).mockResolvedValue(
        'file:///data/history/Image/AABBCC/photo.png'
      );

      const result = await resolveRemoteContent(content, 'AABBCC', null, true, deps);

      expect(result).not.toBeNull();
      expect(result!.isJustUploaded).toBe(false);
      expect(result!.foundInHistory).toBe(true);
      expect(result!.content.fileUri).toBe('file:///data/history/Image/AABBCC/photo.png');
    });
  });

  describe('当图片是刚上传的内容时（核心 Bug 场景）', () => {
    // 场景：用户通过首页菜单上传图片 → 文件已复制到 APP 历史目录 → 服务器上传完成
    // → fetchRemoteClipboard 获取到的 content 没有 fileUri（来自服务器DTO）
    // → isJustUploaded=true，应该查询历史记录获取 fileUri

    const uploadedImageContent: ClipboardContent = {
      type: 'Image',
      text: 'photo.png',
      profileHash: 'AABBCC',
      fileName: 'photo.png',
      fileSize: 5000,
      hasData: true,
      // 注意：没有 fileUri —— 这是从服务器 profileDtoToContent 转换而来的
    };

    it('应从历史记录获取 fileUri，使首页卡片显示为已下载状态', async () => {
      // 模拟：历史记录中已有该文件（上传时 importFileToHistory 已写入）
      (deps.getLastUploadedHash as jest.Mock).mockReturnValue('AABBCC');
      (deps.getHistoryItem as jest.Mock).mockResolvedValue({
        fileUri: 'file:///data/history/Image/AABBCC/photo.png',
      });
      (deps.getHistoryFileUri as jest.Mock).mockResolvedValue(
        'file:///data/history/Image/AABBCC/photo.png'
      );

      const result = await resolveRemoteContent(
        uploadedImageContent,
        'AABBCC',
        null, // previousHash=null 表示首次加载或不同 hash
        true, // hasData
        deps
      );

      expect(result).not.toBeNull();
      expect(result!.isJustUploaded).toBe(true);
      // 关键断言：即使是刚上传的内容，也应从历史记录中获取 fileUri
      expect(result!.content.fileUri).toBe('file:///data/history/Image/AABBCC/photo.png');
    });

    it('应从历史记录获取 fileUri（hash 大小写不同）', async () => {
      // 服务器返回的 hash 可能大小写与本地不同
      (deps.getLastUploadedHash as jest.Mock).mockReturnValue('aabbcc');
      (deps.getHistoryItem as jest.Mock).mockResolvedValue({
        fileUri: 'file:///data/history/Image/aabbcc/photo.png',
      });
      (deps.getHistoryFileUri as jest.Mock).mockResolvedValue(
        'file:///data/history/Image/aabbcc/photo.png'
      );

      const result = await resolveRemoteContent(
        uploadedImageContent,
        'AABBCC', // 大写
        'PREVIOUS_HASH',
        true,
        deps
      );

      expect(result).not.toBeNull();
      expect(result!.isJustUploaded).toBe(true);
      expect(result!.content.fileUri).toBe('file:///data/history/Image/aabbcc/photo.png');
    });

    it('当历史记录中文件不存在时，fileUri 应为 undefined', async () => {
      // 极端情况：上传完成但历史文件被清理了
      (deps.getLastUploadedHash as jest.Mock).mockReturnValue('AABBCC');
      (deps.getHistoryItem as jest.Mock).mockResolvedValue(null);

      const result = await resolveRemoteContent(uploadedImageContent, 'AABBCC', null, true, deps);

      expect(result).not.toBeNull();
      expect(result!.isJustUploaded).toBe(true);
      // 历史记录没有，fileUri 应该为 undefined
      expect(result!.content.fileUri).toBeUndefined();
    });
  });

  describe('当文件是刚上传的内容时', () => {
    const uploadedFileContent: ClipboardContent = {
      type: 'File',
      text: 'document.pdf',
      profileHash: 'DDEEFF',
      fileName: 'document.pdf',
      fileSize: 10000,
      hasData: true,
    };

    it('应从历史记录获取 fileUri', async () => {
      (deps.getLastUploadedHash as jest.Mock).mockReturnValue('DDEEFF');
      (deps.getHistoryItem as jest.Mock).mockResolvedValue({
        fileUri: 'file:///data/history/File/DDEEFF/document.pdf',
      });
      (deps.getHistoryFileUri as jest.Mock).mockResolvedValue(
        'file:///data/history/File/DDEEFF/document.pdf'
      );

      const result = await resolveRemoteContent(
        uploadedFileContent,
        'DDEEFF',
        'OLD_HASH',
        true,
        deps
      );

      expect(result).not.toBeNull();
      expect(result!.isJustUploaded).toBe(true);
      expect(result!.content.fileUri).toBe('file:///data/history/File/DDEEFF/document.pdf');
    });
  });

  describe('当历史查询出错时', () => {
    it('不是刚上传的内容时，应优雅降级返回无 fileUri 的 content', async () => {
      const content: ClipboardContent = {
        type: 'Image',
        text: 'photo.png',
        profileHash: 'AABBCC',
        fileName: 'photo.png',
        fileSize: 5000,
        hasData: true,
      };

      (deps.getHistoryItem as jest.Mock).mockRejectedValue(new Error('DB error'));

      const result = await resolveRemoteContent(content, 'AABBCC', null, true, deps);

      expect(result).not.toBeNull();
      expect(result!.content.fileUri).toBeUndefined();
      expect(result!.foundInHistory).toBe(false);
    });
  });
});
