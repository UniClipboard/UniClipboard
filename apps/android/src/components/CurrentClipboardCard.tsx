/**
 * Current Clipboard Card Component
 * 当前剪贴板内容卡片
 */

import React, { useState, useEffect, useMemo } from 'react';
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  Platform,
  Share,
  Image,
  Linking,
} from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';
import { ClipboardContent } from '@/types/clipboard';
import { useSettingsStore } from '@/stores';
import { useMessageStore } from '@/stores/messageStore';
import { openFile, shareFile, saveFile, saveToGallery } from '@/utils/fileActions';
import { formatFileSize, formatSizeWithType, isTextInvalid } from '@/utils';

interface DownloadProgress {
  progress: number;
  bytesTransferred: number;
  totalBytes: number;
}

interface CurrentClipboardCardProps {
  clipboard: ClipboardContent | null;
  isRemote?: boolean;
  onUpload?: () => void;
  uploading?: boolean;
  onCancelUpload?: () => void;
  onDownload?: () => void;
  downloading?: boolean;
  downloadProgress?: DownloadProgress | null;
  onCancelDownload?: () => void;
  onCopy: (content: ClipboardContent) => Promise<void>;
  onWordPick?: (text: string) => void;
}

export const CurrentClipboardCard: React.FC<CurrentClipboardCardProps> = ({
  clipboard,
  isRemote = false,
  onUpload,
  uploading = false,
  onCancelUpload,
  onDownload,
  downloading = false,
  downloadProgress,
  onCancelDownload,
  onCopy,
  onWordPick,
}) => {
  const { theme } = useTheme();
  const { config } = useSettingsStore();
  const { showMessage } = useMessageStore();
  const isDebugMode = config?.debugMode ?? false;
  const [, setUpdateTrigger] = useState(0);

  // 监控 clipboard 变化并强制更新
  useEffect(() => {
    if (clipboard?.localClipboardHash) {
      console.log('[CurrentClipboardCard] ✓ Received clipboard update:', {
        type: clipboard.type,
        contentHash: clipboard.localClipboardHash.substring(0, 8),
        imageUri: clipboard.fileUri?.substring(clipboard.fileUri.lastIndexOf('/') + 1),
        timestamp: clipboard.timestamp,
      });
      setUpdateTrigger((prev) => prev + 1);
    }
  }, [clipboard?.localClipboardHash, clipboard?.fileUri]);

  // 每 30 秒更新一次时间显示
  useEffect(() => {
    const interval = setInterval(() => {
      setUpdateTrigger((prev) => prev + 1);
    }, 30000);

    return () => clearInterval(interval);
  }, []);

  // 检测文本中的 URL
  const detectedUrl = useMemo(() => {
    if (!clipboard || clipboard.type !== 'Text' || !clipboard.text) return null;
    const urlRegex = /https?:\/\/[^\s<>"'()\]\[{}]+/i;
    const match = clipboard.text.match(urlRegex);
    return match ? match[0] : null;
  }, [clipboard?.type, clipboard?.text]);

  // 分享内容
  const handleShare = async () => {
    if (!clipboard) return;
    try {
      if (clipboard.type === 'Text' && !isTextInvalid(clipboard.text)) {
        await Share.share({ message: clipboard.text });
      } else if (clipboard.fileUri) {
        await shareFile(clipboard.fileUri, clipboard.fileName);
      }
    } catch (error) {
      console.error('[CurrentClipboardCard] Failed to share:', error);
    }
  };

  if (!clipboard) {
    return (
      <View style={[styles.card, { backgroundColor: theme.colors.surfaceContainerLow }]}>
        <View style={styles.emptyContent}>
          <Text style={[styles.emptyIcon, { color: theme.colors.textTertiary }]}>📋</Text>
          <Text style={[styles.emptyTitle, { color: theme.colors.text }]}>剪贴板为空</Text>
          <Text style={[styles.emptyDescription, { color: theme.colors.onSurfaceVariant }]}>
            复制内容后将在此显示
          </Text>
        </View>
      </View>
    );
  }

  const getTypeIcon = (type: string): string => {
    switch (type) {
      case 'Text':
        return '📝';
      case 'Image':
        return '🖼️';
      case 'File':
        return '📄';
      default:
        return '📋';
    }
  };

  const getTypeLabel = (type: string): string => {
    switch (type) {
      case 'Text':
        return '文本';
      case 'Image':
        return '图片';
      case 'File':
        return '文件';
      default:
        return '未知';
    }
  };

  const formatTime = (timestamp: number): string => {
    const now = Date.now();
    const diff = now - timestamp;

    if (diff < 60000) return '刚刚';
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;

    return new Date(timestamp).toLocaleDateString('zh-CN', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  // 获取预览文本
  const getPreviewText = (): string => {
    if (clipboard.type === 'Text') {
      return clipboard.text || '';
    }
    if (clipboard.type === 'Image') {
      return clipboard.fileName || '图片';
    }
    if (clipboard.type === 'File') {
      return clipboard.fileName || '文件';
    }
    return '';
  };

  const previewText = getPreviewText();
  const isLongText = previewText.length > 200;

  // 判断是否需要下载额外文件
  const needsFileDownload = (): boolean => {
    if (!isRemote || !clipboard) return false;

    // 文本类型：当 hasData 为 true 且有 fileName 但没有 fileUri 时，需要下载完整文本
    if (clipboard.type === 'Text') {
      const needsDownload = !!(clipboard.hasData && clipboard.fileName && !clipboard.fileUri);
      return needsDownload;
    }

    // 图片类型：有 fileName 但没有 fileUri 或 fileData
    if (clipboard.type === 'Image') {
      return !!(clipboard.fileName && !clipboard.fileUri && !clipboard.fileData);
    }

    // 文件类型：有 fileName 但没有 fileUri 或 fileData
    if (clipboard.type === 'File') {
      return !!(clipboard.fileName && !clipboard.fileUri && !clipboard.fileData);
    }

    return false;
  };

  const showDownloadButton = isRemote && onDownload && needsFileDownload();

  // 可以"打开"的非文本类型（有 fileUri）
  const canOpenFile = clipboard.type !== 'Text' && !!clipboard.fileUri;

  // 打开文件
  const handleOpenFile = async () => {
    if (!clipboard.fileUri) return;
    try {
      await openFile(clipboard.fileUri);
    } catch (error) {
      console.error('[CurrentClipboardCard] Failed to open file:', error);
    }
  };

  // 判断是否显示分享按钮（非Text类型且有文件URI）
  const canShowShareButton = (() => {
    if (!clipboard || clipboard.type === 'Text') return false;

    // 图片类型：需要有 fileUri
    if (clipboard.type === 'Image') return !!clipboard.fileUri;
    // 文件类型：需要有 fileUri
    if (clipboard.type === 'File') return !!clipboard.fileUri;

    return false;
  })();

  // 判断是否显示保存按钮（非Text类型且有文件URI）
  const canShowSaveButton = canShowShareButton;

  // 保存文件到用户选择的目录（图片类型保存到相册）
  const handleSaveFile = async () => {
    if (!clipboard.fileUri) return;
    try {
      if (clipboard.type === 'Image') {
        await saveToGallery(clipboard.fileUri);
        showMessage('已保存到相册', 'success');
      } else {
        await saveFile(clipboard.fileUri, clipboard.fileName);
        showMessage('已储存到设备', 'success');
      }
    } catch (error) {
      if (error instanceof Error && error.message === 'Media library permission denied') {
        showMessage('需要相册权限才能保存图片', 'error');
        return;
      }
      console.error('[CurrentClipboardCard] Failed to save file:', error);
      showMessage('保存失败', 'error');
    }
  };

  return (
    <View style={[styles.card, { backgroundColor: theme.colors.surfaceContainerHigh }]}>
      {/* 标题栏 — M3 leading avatar + 标题 */}
      <View style={styles.header}>
        <View style={styles.headerLeft}>
          <View style={[styles.leadingAvatar, { backgroundColor: theme.colors.primaryContainer }]}>
            <Text style={styles.typeIcon}>{getTypeIcon(clipboard.type)}</Text>
          </View>
          <View style={styles.headerInfo}>
            <Text style={[styles.typeLabel, { color: theme.colors.text }]}>
              {getTypeLabel(clipboard.type)}
            </Text>
            <Text style={[styles.timestamp, { color: theme.colors.onSurfaceVariant }]}>
              {clipboard.timestamp ? formatTime(clipboard.timestamp) : '刚刚'}
            </Text>
          </View>
        </View>

        {clipboard.fileSize !== undefined && (
          <Text style={[styles.sizeLabel, { color: theme.colors.onSurfaceVariant }]}>
            {formatSizeWithType(clipboard.fileSize, clipboard.type)}
          </Text>
        )}
      </View>

      {/* 内容预览 */}
      <View style={styles.content}>
        {clipboard.type === 'Text' && (
          <Text
            style={[styles.previewText, { color: theme.colors.text }]}
            numberOfLines={isLongText ? 8 : undefined}
          >
            {previewText}
          </Text>
        )}

        {clipboard.type === 'Image' && (
          <View style={styles.mediaPreview}>
            {clipboard.fileUri ? (
              <>
                <Image
                  key={`image-${clipboard.localClipboardHash?.substring(0, 12)}-${
                    clipboard.timestamp
                  }`}
                  source={{
                    uri: `${clipboard.fileUri}?hash=${
                      clipboard.localClipboardHash?.substring(0, 12) ||
                      clipboard.timestamp ||
                      Date.now()
                    }`,
                    cache: 'reload',
                  }}
                  style={styles.imagePreview}
                  resizeMode="contain"
                  onError={(error) => {
                    console.error('[CurrentClipboardCard] Image load error:', error.nativeEvent);
                    console.error('[CurrentClipboardCard] File URI:', clipboard.fileUri);
                    console.error(
                      '[CurrentClipboardCard] Content Hash:',
                      clipboard.localClipboardHash?.substring(0, 8)
                    );
                  }}
                  onLoad={() => {
                    console.log(
                      '[CurrentClipboardCard] Image loaded successfully:',
                      clipboard.fileUri,
                      'contentHash:',
                      clipboard.localClipboardHash?.substring(0, 8)
                    );
                  }}
                />
              </>
            ) : (
              <View>
                <Text style={[styles.mediaLabel, { color: theme.colors.textSecondary }]}>
                  {clipboard.fileName || '图片文件'}
                </Text>
                <Text style={[styles.mediaHint, { color: theme.colors.textTertiary }]}>
                  等待下载...
                </Text>
              </View>
            )}
          </View>
        )}

        {clipboard.type === 'File' && (
          <View style={styles.mediaPreview}>
            <Text style={[styles.mediaLabel, { color: theme.colors.textSecondary }]}>
              {clipboard.fileName || '文件'}
            </Text>
            {clipboard.fileUri && (
              <Text style={[styles.mediaHint, { color: theme.colors.textTertiary }]}>
                包含文件数据
              </Text>
            )}
          </View>
        )}
      </View>

      {/* 按钮区域 — M3 Filled Tonal(主)+ Outlined(次)*/}
      <View style={styles.actionButtons}>
        {/* 文本中包含 URL：打开链接按钮 */}
        {clipboard.type === 'Text' && detectedUrl && (
          <TouchableOpacity
            style={[styles.actionButton, { backgroundColor: theme.colors.primaryContainer }]}
            onPress={() => Linking.openURL(detectedUrl)}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              打开链接
            </Text>
          </TouchableOpacity>
        )}

        {/* 文本类型：分词按钮 */}
        {clipboard.type === 'Text' && onWordPick && !isTextInvalid(clipboard.text) && (
          <TouchableOpacity
            style={[styles.actionButton, { backgroundColor: theme.colors.primaryContainer }]}
            onPress={() => onWordPick(clipboard.text!)}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              分词
            </Text>
          </TouchableOpacity>
        )}

        {/* 远程 Text 类型：只有在不需要下载时才显示复制按钮 */}
        {isRemote && clipboard.type === 'Text' && !showDownloadButton && (
          <TouchableOpacity
            style={[
              styles.actionButton,
              styles.actionButtonLast,
              { backgroundColor: theme.colors.primaryContainer },
            ]}
            onPress={() => onCopy(clipboard)}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              复制
            </Text>
          </TouchableOpacity>
        )}

        {/* 非文本且有文件：打开按钮 */}
        {canOpenFile && (
          <TouchableOpacity
            style={[styles.actionButton, { backgroundColor: theme.colors.primaryContainer }]}
            onPress={handleOpenFile}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              打开
            </Text>
          </TouchableOpacity>
        )}

        {/* 非Text类型且已下载：保存按钮 */}
        {canShowSaveButton && (
          <TouchableOpacity
            style={[styles.actionButton, { backgroundColor: theme.colors.primaryContainer }]}
            onPress={handleSaveFile}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              保存
            </Text>
          </TouchableOpacity>
        )}

        {/* 非Text类型且已下载：分享按钮 */}
        {canShowShareButton && (
          <TouchableOpacity
            style={[
              styles.actionButton,
              styles.actionButtonLast,
              { backgroundColor: theme.colors.primaryContainer },
            ]}
            onPress={handleShare}
          >
            <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
              分享
            </Text>
          </TouchableOpacity>
        )}

        {/* 同步操作按钮 — Outlined */}
        {!isRemote && onUpload && (
          <TouchableOpacity
            style={[
              styles.actionButton,
              styles.secondaryButton,
              styles.actionButtonLast,
              { borderColor: theme.colors.outline },
            ]}
            onPress={uploading ? onCancelUpload : onUpload}
          >
            <Text
              style={[
                styles.actionButtonText,
                styles.secondaryButtonText,
                { color: theme.colors.primary },
              ]}
            >
              {uploading ? '取消' : '上传'}
            </Text>
          </TouchableOpacity>
        )}

        {showDownloadButton && (
          <TouchableOpacity
            style={[
              styles.actionButton,
              styles.secondaryButton,
              styles.actionButtonLast,
              { borderColor: theme.colors.outline },
            ]}
            onPress={downloading ? onCancelDownload : onDownload}
          >
            {downloading && downloadProgress && (
              <View
                style={[
                  styles.progressFill,
                  {
                    backgroundColor: theme.colors.primary,
                    width: `${downloadProgress.progress * 100}%`,
                  },
                ]}
              />
            )}
            <Text
              style={[
                styles.actionButtonText,
                styles.secondaryButtonText,
                { color: theme.colors.primary },
              ]}
            >
              {downloading && downloadProgress
                ? `${(downloadProgress.progress * 100).toFixed(0)}%  ${formatFileSize(downloadProgress.bytesTransferred)} / ${formatFileSize(downloadProgress.totalBytes)}`
                : downloading
                  ? '取消'
                  : '下载'}
            </Text>
          </TouchableOpacity>
        )}
      </View>

      {/* Hash 信息 */}
      {isDebugMode && clipboard.profileHash && (
        <View style={[styles.footer, { borderTopColor: theme.colors.divider }]}>
          <Text style={[styles.hashLabel, { color: theme.colors.textTertiary }]}>
            Hash: {clipboard.profileHash.substring(0, 16)}...
          </Text>
        </View>
      )}
    </View>
  );
};

const styles = StyleSheet.create({
  card: {
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    padding: spacing.base,
    ...elevation.sm,
  },
  emptyContent: {
    alignItems: 'center',
    paddingVertical: spacing.xxl,
  },
  emptyIcon: {
    fontSize: 48,
    marginBottom: spacing.md,
  },
  emptyTitle: {
    fontSize: typography.title3.fontSize,
    lineHeight: typography.title3.lineHeight,
    fontWeight: typography.title3.fontWeight,
    marginBottom: spacing.xs,
  },
  emptyDescription: {
    fontSize: typography.subhead.fontSize,
    lineHeight: typography.subhead.lineHeight,
  },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    marginBottom: spacing.md,
  },
  headerLeft: {
    flexDirection: 'row',
    alignItems: 'center',
    flex: 1,
  },
  leadingAvatar: {
    width: 40,
    height: 40,
    borderRadius: radius.pill,
    alignItems: 'center',
    justifyContent: 'center',
    marginRight: spacing.md,
  },
  typeIcon: {
    fontSize: 20,
  },
  headerInfo: {
    flex: 1,
  },
  typeLabel: {
    fontSize: typography.headline.fontSize,
    lineHeight: typography.headline.lineHeight,
    fontWeight: typography.headline.fontWeight,
    marginBottom: 2,
  },
  timestamp: {
    fontSize: typography.footnote.fontSize,
    lineHeight: typography.footnote.lineHeight,
  },
  sizeLabel: {
    fontSize: typography.footnote.fontSize,
    fontWeight: '500',
  },
  content: {
    marginBottom: spacing.base,
  },
  actionButtons: {
    flexDirection: 'row',
    marginBottom: spacing.md,
    gap: spacing.sm,
  },
  actionButton: {
    flex: 1,
    paddingVertical: spacing.sm + 2,
    paddingHorizontal: spacing.base,
    borderRadius: radius.pill,
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'hidden',
    minHeight: 40,
  },
  actionButtonLast: {},
  secondaryButton: {
    borderWidth: 1,
  },
  actionButtonText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
  },
  secondaryButtonText: {
    fontWeight: '500',
  },
  previewText: {
    fontSize: typography.subhead.fontSize,
    lineHeight: 22,
  },
  mediaPreview: {
    paddingVertical: spacing.sm,
  },
  imagePreview: {
    width: '100%',
    height: 200,
    borderRadius: radius.lg,
    borderCurve: 'continuous',
  },
  mediaLabel: {
    fontSize: typography.subhead.fontSize,
    marginBottom: spacing.xs,
  },
  mediaHint: {
    fontSize: typography.footnote.fontSize,
  },
  footer: {
    paddingTop: spacing.md,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  hashLabel: {
    fontSize: 11,
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
  },
  progressFill: {
    position: 'absolute',
    left: 0,
    top: 0,
    bottom: 0,
    opacity: 0.15,
  },
});
