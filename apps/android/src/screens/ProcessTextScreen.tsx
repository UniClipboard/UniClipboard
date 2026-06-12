/**
 * Process Text Screen
 * 处理来自 Android 文字选中菜单（PROCESS_TEXT）的上传请求。
 * 复用 QuickLoadingPage 和 uploadTextAndAddToHistory，与 ShareReceiveScreen 保持一致。
 */

import React, { useCallback } from 'react';
import { QuickLoadingPage } from '@/components/QuickLoadingPage';
import { useSettingsStore } from '@/stores/settingsStore';
import { uploadTextAndAddToHistory } from '@/utils/uploadFile';

interface ProcessTextScreenProps {
  text: string;
  onComplete: () => void;
}

export const ProcessTextScreen: React.FC<ProcessTextScreenProps> = ({ text, onComplete }) => {
  const activeServer = useSettingsStore((s) => s.getActiveServer());

  const task = useCallback(
    async (signal: AbortSignal) => {
      if (!activeServer) throw new Error('请先在设置中配置服务器');
      await uploadTextAndAddToHistory(text, activeServer, { signal });
    },
    [text, activeServer]
  );

  return (
    <QuickLoadingPage
      task={task}
      loadingText="正在上传文字…"
      successText="上传成功"
      failureText="上传失败"
      onComplete={onComplete}
      previewText={text.length > 50 ? `${text.slice(0, 50)}…` : text}
    />
  );
};
