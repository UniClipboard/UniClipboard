/**
 * QuickLoadingPage
 * 通用"快速加载"页：执行一个异步 task，处理 loading / success / error 状态显示。
 * 纯 UI + 状态机，不含任何业务逻辑。
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  View,
  StyleSheet,
  ActivityIndicator,
  Text,
  Image,
  TouchableOpacity,
  TouchableWithoutFeedback,
  BackHandler,
  ScrollView,
} from 'react-native';
import * as Clipboard from 'expo-clipboard';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';
import type { ClipboardContent } from '@/types/clipboard';
import type { ProgressInfo } from 'native-util';
import { formatFileSize, isTextInvalid } from '@/utils';

type LoadingState = 'loading' | 'success' | 'error';

export interface SuccessButtonConfig {
  label: string;
  onPress: () => void;
  primary?: boolean;
}

export interface QuickLoadingPageProps {
  task: (signal: AbortSignal) => Promise<void>;
  loadingText: string;
  successText: string;
  failureText: string;
  onComplete: () => void;
  successContent?: ClipboardContent;
  successButtons?: SuccessButtonConfig[];
  progress?: ProgressInfo | null;
  previewText?: string;
  previewImage?: string;
  /** When true, renders as a floating card over a semi-transparent backdrop (for transparent Activity). */
  overlayMode?: boolean;
}

export const QuickLoadingPage: React.FC<QuickLoadingPageProps> = ({
  task,
  loadingText,
  successText,
  failureText,
  onComplete,
  successContent,
  successButtons,
  progress,
  previewText,
  previewImage,
  overlayMode,
}) => {
  const { theme } = useTheme();
  const [state, setState] = useState<LoadingState>('loading');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  // 用 ref 持有 task，避免 task 引用变化触发 useEffect 重复执行
  const taskRef = useRef(task);
  useEffect(() => {
    taskRef.current = task;
  }, [task]);

  // AbortController 用于取消任务
  const abortControllerRef = useRef<AbortController | null>(null);

  const run = useCallback(async () => {
    setState('loading');
    setErrorMessage(null);

    // 创建新的 AbortController
    abortControllerRef.current = new AbortController();
    const signal = abortControllerRef.current.signal;

    try {
      await taskRef.current(signal);
      setState('success');
    } catch (err) {
      // 如果是取消操作，直接返回，不显示错误
      if (signal.aborted) {
        return;
      }
      setErrorMessage(err instanceof Error ? err.message : '操作失败，请重试');
      setState('error');
    }
  }, [onComplete]);

  useEffect(() => {
    run();
    return () => {
      // 组件卸载时取消任务
      abortControllerRef.current?.abort();
    };
  }, [run]);

  // 取消任务
  const handleCancel = useCallback(() => {
    abortControllerRef.current?.abort();
    onComplete();
  }, [onComplete]);

  // 成功后：无 successContent 且无 successButtons 时自动关闭
  // 放在独立 useEffect 中，确保在 React 批处理完成、父组件更新 successButtons prop 后再判断
  useEffect(() => {
    if (state !== 'success') return;
    if (successContent !== undefined || (successButtons && successButtons.length > 0)) return;
    // 无需显示成功界面，直接退出
    onComplete();
  }, [state, successContent, successButtons, onComplete]);

  // 返回键：loading 时允许取消；error / success-with-content/extra 时允许离开
  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', () => {
      if (state === 'loading') {
        handleCancel();
        return true;
      }
      if (
        state === 'error' ||
        (state === 'success' &&
          (successContent !== undefined || (successButtons && successButtons.length > 0)))
      ) {
        onComplete();
        return true;
      }
      return false;
    });
    return () => sub.remove();
  }, [state, successContent, successButtons, onComplete, handleCancel]);

  const containerBg = overlayMode
    ? { backgroundColor: 'transparent' }
    : { backgroundColor: theme.colors.surface };

  const progressFillDynamic = useMemo(
    () => ({
      backgroundColor: theme.colors.primary,
      width:
        (progress?.totalBytes ?? 0) > 0
          ? (`${(progress?.progress ?? 0) * 100}%` as const)
          : ('100%' as const),
    }),
    [theme.colors.primary, progress?.totalBytes, progress?.progress]
  );

  const contentView = (
    <View
      style={[
        styles.content,
        overlayMode && [styles.overlayCard, { backgroundColor: theme.colors.surfaceContainerHigh }],
      ]}
    >
      {state === 'loading' && (
        <>
          <ActivityIndicator size="large" color={theme.colors.primary} />
          <Text style={[styles.statusText, { color: theme.colors.text }]}>{loadingText}</Text>
          {previewImage && (
            <Image source={{ uri: previewImage }} style={styles.loadingPreviewImage} />
          )}
          {previewText && (
            <Text style={[styles.loadingPreviewText, { color: theme.colors.textSecondary }]}>
              {previewText}
            </Text>
          )}
          {progress && (progress.totalBytes > 0 || progress.bytesTransferred > 0) && (
            <View style={styles.progressContainer}>
              <View style={[styles.progressBar, { backgroundColor: theme.colors.border }]}>
                <View
                  style={[
                    styles.progressFill,
                    progressFillDynamic,
                    progress.totalBytes <= 0 && styles.progressFillIndeterminate,
                  ]}
                />
              </View>
              <Text style={[styles.progressText, { color: theme.colors.textSecondary }]}>
                {progress.totalBytes > 0
                  ? `${(progress.progress * 100).toFixed(0)}% ${formatFileSize(progress.bytesTransferred)} / ${formatFileSize(progress.totalBytes)}`
                  : formatFileSize(progress.bytesTransferred)}
              </Text>
            </View>
          )}
          <TouchableOpacity
            style={[
              styles.button,
              styles.buttonOutline,
              { backgroundColor: 'transparent', borderColor: theme.colors.outline },
            ]}
            onPress={handleCancel}
          >
            <Text style={[styles.buttonText, { color: theme.colors.primary }]}>取消</Text>
          </TouchableOpacity>
        </>
      )}

      {state === 'success' && (
        <>
          {successContent && <ContentPreview content={successContent} />}
          {!(successButtons && successButtons.length > 0) && (
            <>
              <Text style={[styles.successIcon, { color: theme.colors.success }]}>✓</Text>
              <Text style={[styles.statusText, { color: theme.colors.text }]}>{successText}</Text>
            </>
          )}
          {(successContent !== undefined || (successButtons && successButtons.length > 0)) && (
            <View style={styles.successButtonRow}>
              {successButtons?.map((btn, index) => (
                <TouchableOpacity
                  key={index}
                  style={[
                    styles.button,
                    styles.successButton,
                    btn.primary
                      ? { backgroundColor: theme.colors.primaryContainer }
                      : [
                          styles.buttonOutline,
                          {
                            backgroundColor: 'transparent',
                            borderColor: theme.colors.outline,
                          },
                        ],
                  ]}
                  onPress={btn.onPress}
                >
                  <Text
                    style={[
                      styles.buttonText,
                      {
                        color: btn.primary ? theme.colors.onPrimaryContainer : theme.colors.primary,
                      },
                    ]}
                  >
                    {btn.label}
                  </Text>
                </TouchableOpacity>
              ))}
              <TouchableOpacity
                style={[
                  styles.button,
                  styles.successButton,
                  styles.buttonOutline,
                  { backgroundColor: 'transparent', borderColor: theme.colors.outline },
                ]}
                onPress={onComplete}
              >
                <Text style={[styles.buttonText, { color: theme.colors.primary }]}>返回</Text>
              </TouchableOpacity>
            </View>
          )}
        </>
      )}

      {state === 'error' && (
        <>
          <Text style={[styles.errorIcon, { color: theme.colors.error }]}>✗</Text>
          <Text style={[styles.statusText, { color: theme.colors.text }]}>{failureText}</Text>
          {errorMessage && (
            <ScrollView
              style={[
                styles.errorDetailScroll,
                {
                  borderColor: theme.colors.outlineVariant,
                  backgroundColor: theme.colors.errorContainer,
                },
              ]}
              contentContainerStyle={styles.errorDetailScrollContent}
            >
              <Text style={[styles.errorDetailText, { color: theme.colors.onErrorContainer }]}>
                {errorMessage}
              </Text>
            </ScrollView>
          )}
          <View style={styles.buttonRow}>
            <TouchableOpacity
              style={[styles.button, { backgroundColor: theme.colors.primaryContainer }]}
              onPress={run}
            >
              <Text style={[styles.buttonText, { color: theme.colors.onPrimaryContainer }]}>
                重试
              </Text>
            </TouchableOpacity>
            {errorMessage && (
              <TouchableOpacity
                style={[
                  styles.button,
                  styles.buttonOutline,
                  { backgroundColor: 'transparent', borderColor: theme.colors.outline },
                ]}
                onPress={() => Clipboard.setStringAsync(errorMessage)}
              >
                <Text style={[styles.buttonText, { color: theme.colors.primary }]}>复制</Text>
              </TouchableOpacity>
            )}
            <TouchableOpacity
              style={[
                styles.button,
                styles.buttonOutline,
                { backgroundColor: 'transparent', borderColor: theme.colors.outline },
              ]}
              onPress={onComplete}
            >
              <Text style={[styles.buttonText, { color: theme.colors.primary }]}>返回</Text>
            </TouchableOpacity>
          </View>
        </>
      )}
    </View>
  );

  return (
    <View style={[styles.container, containerBg]}>
      {overlayMode ? (
        <TouchableWithoutFeedback
          onPress={() => {
            if (state !== 'loading') onComplete();
          }}
        >
          <View style={styles.overlayBackdrop}>{contentView}</View>
        </TouchableWithoutFeedback>
      ) : (
        contentView
      )}
    </View>
  );
};

// ---------------------------------------------------------------------------
// ContentPreview – inline preview of a ClipboardContent result
// ---------------------------------------------------------------------------

const ContentPreview: React.FC<{ content: ClipboardContent }> = ({ content }) => {
  const { theme } = useTheme();

  if (content.type === 'Image' && content.fileUri) {
    return (
      <Image source={{ uri: content.fileUri }} style={styles.previewImage} resizeMode="contain" />
    );
  }

  if (content.type === 'Text' && !isTextInvalid(content.text)) {
    return (
      <View
        style={[
          styles.previewTextBox,
          { backgroundColor: theme.colors.background, borderColor: theme.colors.border },
        ]}
      >
        <Text
          style={[styles.previewText, { color: theme.colors.text }]}
          numberOfLines={6}
          ellipsizeMode="tail"
        >
          {content.text}
        </Text>
      </View>
    );
  }

  // File (or Image without local URI)
  const label = content.fileName ?? content.text ?? '未知文件';
  const size = content.fileSize != null ? ` · ${(content.fileSize / 1024).toFixed(1)} KB` : '';
  return (
    <View
      style={[
        styles.previewFileBox,
        { backgroundColor: theme.colors.background, borderColor: theme.colors.border },
      ]}
    >
      <Text style={[styles.previewFileIcon, { color: theme.colors.primary }]}>📄</Text>
      <Text style={[styles.previewFileName, { color: theme.colors.text }]} numberOfLines={2}>
        {label}
      </Text>
      {size !== '' && (
        <Text style={[styles.previewFileMeta, { color: theme.colors.textTertiary }]}>
          {size.trim()}
        </Text>
      )}
    </View>
  );
};

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
  },
  content: {
    alignItems: 'center',
    alignSelf: 'stretch',
    paddingHorizontal: spacing.xl,
    gap: spacing.base,
  },
  statusText: {
    fontSize: typography.callout.fontSize,
  },
  successIcon: {
    fontSize: 48,
  },
  errorIcon: {
    fontSize: 48,
  },
  errorDetailScroll: {
    maxHeight: 200,
    width: '100%',
    maxWidth: 280,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  errorDetailScrollContent: {
    padding: spacing.md,
  },
  errorDetailText: {
    fontSize: typography.footnote.fontSize,
    textAlign: 'center',
  },
  buttonRow: {
    flexDirection: 'row',
    gap: spacing.md,
    marginTop: spacing.sm,
  },
  successButtonRow: {
    flexDirection: 'row',
    gap: spacing.md,
    marginTop: spacing.sm,
    width: '100%',
  },
  successButton: {
    flex: 1,
    paddingHorizontal: 0,
  },
  button: {
    paddingHorizontal: spacing.xl,
    paddingVertical: spacing.sm + 2,
    borderRadius: radius.pill,
    minHeight: 40,
    alignItems: 'center',
    justifyContent: 'center',
  },
  buttonText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
    textAlign: 'center',
  },
  buttonOutline: {
    borderWidth: 1,
  },
  loadingPreviewText: {
    fontSize: 14,
    textAlign: 'center',
    maxWidth: 280,
  },
  loadingPreviewImage: {
    width: 120,
    height: 120,
    borderRadius: 8,
    resizeMode: 'cover',
  },
  progressContainer: {
    width: '100%',
    maxWidth: 280,
    alignItems: 'center',
    gap: 8,
  },
  progressBar: {
    width: '100%',
    height: 8,
    borderRadius: 4,
    overflow: 'hidden',
  },
  progressFill: {
    height: '100%',
    borderRadius: 4,
  },
  progressFillIndeterminate: {
    opacity: 0.3,
  },
  progressText: {
    fontSize: 13,
  },
  previewImage: {
    width: '100%',
    aspectRatio: 1,
    maxHeight: 320,
    borderRadius: 12,
  },
  previewTextBox: {
    width: 280,
    maxHeight: 160,
    borderRadius: 8,
    borderWidth: 1,
    padding: 12,
  },
  previewText: {
    fontSize: 14,
    lineHeight: 20,
  },
  previewFileBox: {
    width: 280,
    borderRadius: 8,
    borderWidth: 1,
    padding: 12,
    alignItems: 'center',
    gap: 6,
  },
  previewFileIcon: {
    fontSize: 32,
  },
  previewFileName: {
    fontSize: 14,
    fontWeight: '500',
    textAlign: 'center',
  },
  previewFileMeta: {
    fontSize: 12,
  },
  overlayBackdrop: {
    flex: 1,
    width: '100%',
    justifyContent: 'center',
    alignItems: 'center',
  },
  overlayCard: {
    borderRadius: radius.xl,
    borderCurve: 'continuous',
    paddingVertical: spacing.xl + spacing.xs,
    paddingHorizontal: spacing.xl,
    width: '85%',
    alignSelf: 'center',
    ...elevation.lg,
  },
});
