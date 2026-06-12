import React, { useCallback, useMemo, useState } from 'react';
import { ToastAndroid, Linking } from 'react-native';
import { SyncDirection } from '@/types/sync';
import { ClipboardContent } from '@/types/clipboard';
import { SyncManager } from '@/services/SyncManager';
import { useSyncStore } from '@/stores/syncStore';
import { openFile, shareFile, saveFile, saveToGallery } from '@/utils/fileActions';
import { isTextInvalid } from '@/utils/index';
import { QuickLoadingPage, SuccessButtonConfig } from '@/components/QuickLoadingPage';
import type { ProgressInfo } from 'native-util';
import * as Clipboard from 'expo-clipboard';

interface QuickTileLoadingScreenProps {
  direction: SyncDirection;
  onLoadingComplete: () => void;
  overlayMode?: boolean;
}

export const QuickTileLoadingScreen: React.FC<QuickTileLoadingScreenProps> = ({
  direction,
  onLoadingComplete,
  overlayMode,
}) => {
  const isUpload = direction === SyncDirection.Upload;

  // 用 state 存储下载的文件内容，触发重渲染以更新 successButtons prop
  const [fileContent, setFileContent] = useState<ClipboardContent | null>(null);
  const [progress, setProgress] = useState<ProgressInfo | null>(null);
  const [previewText, setPreviewText] = useState<string | undefined>(undefined);

  const task = useCallback(
    async (signal: AbortSignal) => {
      setFileContent(null);
      setProgress(null);
      setPreviewText(undefined);

      // 确保 SyncManager 已初始化（冷启动时尚未经过正常启动流程）
      await useSyncStore.getState().initialize();
      const initError = useSyncStore.getState().error;
      if (initError) throw new Error(initError);

      const syncMgr = SyncManager.getInstance();
      const result = await syncMgr.sync(
        direction,
        false,
        signal,
        (info) => setProgress(info),
        (preview) => setPreviewText(preview)
      );

      if (!result.success) {
        throw new Error(result.error || (isUpload ? '上传失败' : '同步失败'));
      }

      const content = result.content;

      // 只有文本类型才显示 Toast 提示
      if (content && content.type === 'Text' && !isTextInvalid(content.text)) {
        const preview = content.text.trim().replace(/\s+/g, ' ');
        const toastMessage = preview.length > 40 ? preview.slice(0, 40) + '…' : preview;
        ToastAndroid.show(toastMessage, ToastAndroid.SHORT);

        // 文本中包含 URL 时，存入 state 以显示操作按钮
        const urlRegex = /https?:\/\/[^\s<>"'()\]\[{}]+/i;
        const urlMatch = content.text.match(urlRegex);
        if (urlMatch) {
          setFileContent(content);
        }
      }

      // 下载了非文本文件时，存入 state，触发重渲染更新 successButtons
      if (!isUpload && content && content.type !== 'Text' && content.fileUri) {
        setFileContent(content);
      }
    },
    [direction, isUpload]
  );

  // 检测文本中的 URL
  const textUrl = useMemo(() => {
    if (!fileContent || fileContent.type !== 'Text' || !fileContent.text) return null;
    const urlRegex = /https?:\/\/[^\s<>"'()\]\[{}]+/i;
    const match = fileContent.text.match(urlRegex);
    return match ? match[0] : null;
  }, [fileContent]);

  const successButtons: SuccessButtonConfig[] | undefined = fileContent
    ? fileContent.type === 'Text' && textUrl
      ? [
          {
            label: '复制',
            primary: true,
            onPress: async () => {
              try {
                await Clipboard.setStringAsync(fileContent.text!);
                ToastAndroid.show('已复制', ToastAndroid.SHORT);
              } catch {}
            },
          },
          {
            label: '打开链接',
            primary: true,
            onPress: async () => {
              try {
                await Linking.openURL(textUrl);
              } catch {}
            },
          },
        ]
      : [
          {
            label: '打开',
            primary: true,
            onPress: async () => {
              try {
                await openFile(fileContent.fileUri!);
              } catch {}
            },
          },
          {
            label: '保存',
            primary: true,
            onPress: async () => {
              try {
                if (fileContent.type === 'Image') {
                  await saveToGallery(fileContent.fileUri!);
                  ToastAndroid.show('已保存到相册', ToastAndroid.SHORT);
                } else {
                  await saveFile(fileContent.fileUri!, fileContent.fileName);
                  ToastAndroid.show('已储存到设备', ToastAndroid.SHORT);
                }
              } catch (error) {
                console.error('[QuickTileLoadingScreen] Failed to save file:', error);
                if (error instanceof Error && error.message === 'Media library permission denied') {
                  ToastAndroid.show('需要相册权限才能保存图片', ToastAndroid.SHORT);
                  return;
                }
                ToastAndroid.show('保存失败', ToastAndroid.SHORT);
              }
            },
          },
          {
            label: '分享',
            primary: true,
            onPress: async () => {
              try {
                await shareFile(fileContent.fileUri!, fileContent.fileName);
              } catch {}
            },
          },
        ]
    : undefined;

  return (
    <QuickLoadingPage
      task={task}
      loadingText={isUpload ? '正在上传剪贴板...' : '正在下载剪贴板...'}
      successText={isUpload ? '上传成功！' : '同步成功！'}
      failureText={isUpload ? '上传失败' : '同步失败'}
      onComplete={onLoadingComplete}
      successContent={fileContent ?? undefined}
      successButtons={successButtons}
      progress={progress}
      previewText={previewText}
      overlayMode={overlayMode}
    />
  );
};
