/**
 * History List Item Component
 * 历史记录列表项组件
 */

import React, { useState, forwardRef, useMemo, useEffect, useRef } from 'react';
import { View, Text, StyleSheet, Pressable, Image, TouchableOpacity, Linking } from 'react-native';
import { Copy, Download, Share, Link2, Scissors } from 'react-native-feather';
import { Ionicons, MaterialCommunityIcons } from '@expo/vector-icons';
import { StarIcon } from './StarIcon';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, elevation } from '@/theme';
import { ClipboardItem } from '@/types/clipboard';
import { useTransferQueueStore } from '@/stores/transferQueueStore';
import { getHistoryTransferQueue } from '@/services/HistoryTransferQueue';
import { getProfileId } from '@/services/HistoryAPI';
import { formatSizeWithType, formatFileSize } from '@/utils';
import { useSettingsStore } from '@/stores';

const ACTION_ICON_SIZE = 15;

interface HistoryListItemProps {
  item: ClipboardItem;
  onCopy: (item: ClipboardItem) => void;
  onShare: (item: ClipboardItem) => void;
  onSave?: (item: ClipboardItem) => void;
  onOpen?: (item: ClipboardItem) => void;
  onLongPress: (item: ClipboardItem) => void;
  onPress?: (item: ClipboardItem) => void;
  onToggleStar?: (item: ClipboardItem) => void;
  onDownload?: (item: ClipboardItem) => void;
  onUpload?: (item: ClipboardItem) => void;
  onWordPick?: (text: string) => void;
  showFullImage?: boolean;
  showDebugInfo?: boolean;
  enableHistorySync?: boolean;
  isSelected?: boolean;
  isMultiSelectMode?: boolean;
  showImageCopyButton?: boolean;
}

export const HistoryListItem = forwardRef<object, HistoryListItemProps>(
  (
    {
      item,
      onCopy,
      onShare,
      onSave,
      onOpen,
      onLongPress,
      onPress,
      onToggleStar,
      onDownload,
      onUpload,
      onWordPick,
      showFullImage = false,
      showDebugInfo = false,
      enableHistorySync = true,
      isSelected = false,
      isMultiSelectMode = false,
      showImageCopyButton = false,
    },
    _ref
  ) => {
    const { theme } = useTheme();
    const tasks = useTransferQueueStore((state) => state.tasks);

    // 响应式读取自动下载设置，变化时触发 effect 重新执行
    const imageAutoDownload = useSettingsStore(
      (state) => state.config?.historyImageAutoDownload ?? 'wifi'
    );

    // 组件挂载或 item/设置 变化时触发自动下载（FlashList 复用场景）
    const autoDownloadTriggered = useRef(false);
    useEffect(() => {
      autoDownloadTriggered.current = false;
    }, [item.profileHash, imageAutoDownload]);

    useEffect(() => {
      if (autoDownloadTriggered.current) return;
      if (item.type !== 'Image' || item.isLocalFileReady !== false || !item.hasRemoteData) return;
      if (!enableHistorySync) return;
      if (imageAutoDownload === 'off') return;

      const doAutoDownload = async () => {
        autoDownloadTriggered.current = true;

        try {
          const netInfo = await (await import('@react-native-community/netinfo')).default.fetch();
          const isWifiNow = netInfo.type === 'wifi';
          if (imageAutoDownload === 'wifi' && !isWifiNow) return;

          const pid = getProfileId(item.type, item.profileHash);
          const queue = getHistoryTransferQueue();
          queue.start();
          await queue.addDownloadTask(pid, true);
        } catch {
          autoDownloadTriggered.current = false;
        }
      };
      doAutoDownload();
    }, [
      item.profileHash,
      item.isLocalFileReady,
      item.type,
      item.hasRemoteData,
      enableHistorySync,
      imageAutoDownload,
    ]);

    const profileId = getProfileId(item.type, item.profileHash);
    const activeTask = tasks.find(
      (t) => t.profileId === profileId && (t.status === 'running' || t.status === 'pending')
    );
    const isTransferring = !!activeTask;
    const transferProgress = activeTask?.progress || 0;
    const isUploadTask = activeTask?.type === 'upload';
    const bytesTransferred = activeTask?.bytesTransferred || 0;
    const totalBytes = activeTask?.totalBytes;

    const handleCancelTransfer = () => {
      if (activeTask) {
        const queue = getHistoryTransferQueue();
        queue.cancelTask(activeTask.profileId, activeTask.type);
      }
    };

    const [imageDimensions, setImageDimensions] = useState<{
      width: number;
      height: number;
    } | null>(null);
    const [containerWidth, setContainerWidth] = useState<number>(0);

    const getTypeIcon = (type: string): string => {
      switch (type) {
        case 'Text':
          return '📝';
        case 'Image':
          return '🖼️';
        case 'File':
          return '📄';
        case 'Group':
          return '📦';
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
        case 'Group':
          return '文件组';
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
      if (diff < 604800000) return `${Math.floor(diff / 86400000)} 天前`;

      const date = new Date(timestamp);
      return date.toLocaleDateString('zh-CN', {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
      });
    };

    const getPreviewText = (): string => {
      if (item.type === 'Text') {
        return item.text || '';
      }
      if (item.type === 'Image') {
        // Image: isLocalFileReady 为 false 时显示 text，为 true 时不显示文本
        return item.text || '图片';
      }
      if (item.type === 'File') {
        // File: 无论 isLocalFileReady 为 true 还是 false 都显示 text
        return item.text || '文件';
      }
      if (item.type === 'Group') {
        return item.text || '文件组';
      }
      return '';
    };

    const previewText = getPreviewText();

    // 检测文本中的 URL
    const detectedUrl = useMemo(() => {
      if (item.type !== 'Text' || !item.text) return null;
      const urlRegex = /https?:\/\/[^\s<>"'()\]\[{}]+/i;
      const match = item.text.match(urlRegex);
      return match ? match[0] : null;
    }, [item.type, item.text]);

    return (
      <Pressable
        onLongPress={() => onLongPress(item)}
        onPress={isMultiSelectMode ? () => onPress?.(item) : undefined}
        style={({ pressed }) => [
          styles.touchable,
          isMultiSelectMode && styles.touchableMultiSelect,
          pressed && !isMultiSelectMode && { opacity: 0.7 },
        ]}
      >
        <View style={isMultiSelectMode ? styles.multiSelectRow : undefined}>
          {/* 多选模式勾选框 - 在 item 边框外 */}
          {isMultiSelectMode && (
            <View style={styles.checkboxContainer}>
              <Ionicons
                name={isSelected ? 'checkbox' : 'square-outline'}
                size={22}
                color={isSelected ? theme.colors.primary : theme.colors.textTertiary}
              />
            </View>
          )}
          <View
            style={[
              styles.container,
              {
                backgroundColor: isSelected
                  ? theme.colors.primaryContainer
                  : theme.colors.surfaceContainerLow,
              },
              isSelected && {
                borderColor: theme.colors.primary,
                borderWidth: 1.5,
              },
              isMultiSelectMode && styles.containerMultiSelect,
            ]}
          >
            {/* 顶部内容区 */}
            <View style={styles.topContent}>
              {/* 左侧图标 — M3 tonal leading */}
              <View
                style={[styles.iconContainer, { backgroundColor: theme.colors.primaryContainer }]}
              >
                <Text style={styles.typeIcon}>{getTypeIcon(item.type)}</Text>
              </View>

              {/* 类型标签和时间 */}
              <Text
                style={[styles.typeLabel, styles.typeLabelSpacing, { color: theme.colors.primary }]}
              >
                {getTypeLabel(item.type)}
              </Text>

              {/* 时间戳 */}
              <Text
                style={[
                  styles.timestamp,
                  styles.timestampAlign,
                  { color: theme.colors.textSecondary },
                ]}
              >
                {formatTime(item.timestamp)}
              </Text>
            </View>

            {/* 预览文本 - 另起一行（文本类型始终显示，图片/文件类型在本地文件未就绪时显示） */}
            {(item.type === 'Text' ||
              item.type === 'File' ||
              (item.type === 'Image' && item.isLocalFileReady === false)) && (
              <Text
                style={[styles.previewText, { color: theme.colors.text }]}
                numberOfLines={10}
                ellipsizeMode="tail"
              >
                {previewText}
              </Text>
            )}

            {/* 图片预览 - 占据整个宽度（仅当有本地文件时显示） */}
            {item.type === 'Image' && item.fileUri && (
              <View
                style={styles.imagePreviewContainer}
                onLayout={(e) => setContainerWidth(e.nativeEvent.layout.width)}
              >
                {showFullImage ? (
                  <Image
                    source={{ uri: item.fileUri }}
                    style={[
                      styles.imagePreview,
                      imageDimensions && containerWidth > 0
                        ? {
                            height:
                              (containerWidth / imageDimensions.width) * imageDimensions.height,
                          }
                        : styles.imagePreviewLimited,
                    ]}
                    resizeMode="cover"
                    onLoad={(e) => {
                      const { width, height } = e.nativeEvent.source;
                      setImageDimensions({ width, height });
                    }}
                  />
                ) : (
                  <Image
                    source={{ uri: item.fileUri }}
                    style={[styles.imagePreview, styles.imagePreviewLimited]}
                    resizeMode="cover"
                  />
                )}
              </View>
            )}

            {/* 底部信息区 */}
            <View style={styles.bottomContent}>
              <View style={styles.metaInfo}>
                {item.size !== undefined && (
                  <Text style={[styles.metaText, { color: theme.colors.textTertiary }]}>
                    {formatSizeWithType(item.size, item.type)}
                  </Text>
                )}
                {/* 传输中状态 - 复用按钮显示取消 */}
                {enableHistorySync && isTransferring && (
                  <TouchableOpacity style={styles.syncBadge} onPress={handleCancelTransfer}>
                    <Ionicons
                      name="close-circle"
                      size={14}
                      color={theme.colors.error || '#F44336'}
                    />
                    <Text
                      style={[styles.syncBadgeText, { color: theme.colors.error || '#F44336' }]}
                    >
                      {transferProgress > 0
                        ? `${Math.round(transferProgress)}%${
                            totalBytes
                              ? ` (${formatFileSize(bytesTransferred)}/${formatFileSize(
                                  totalBytes
                                )})`
                              : ''
                          }`
                        : isUploadTask
                          ? '取消上传'
                          : '取消下载'}
                    </Text>
                  </TouchableOpacity>
                )}
                {/* 同步状态显示，仅当启用同步且不在传输中时显示 */}
                {enableHistorySync && !isTransferring && item.syncStatus !== undefined && (
                  <TouchableOpacity style={styles.syncBadge} onPress={() => onUpload?.(item)}>
                    <Ionicons
                      name={
                        item.syncStatus === 1
                          ? 'cloud-done'
                          : item.syncStatus === 0
                            ? 'cloud-upload'
                            : 'cloud-download'
                      }
                      size={14}
                      color={
                        item.syncStatus === 1
                          ? theme.colors.success || '#4CAF50'
                          : theme.colors.textTertiary
                      }
                    />
                    <Text
                      style={[
                        styles.syncBadgeText,
                        {
                          color:
                            item.syncStatus === 1
                              ? theme.colors.success || '#4CAF50'
                              : theme.colors.textTertiary,
                        },
                      ]}
                    >
                      {item.syncStatus === 1
                        ? '已同步'
                        : item.syncStatus === 0
                          ? '仅本地'
                          : '待同步'}
                    </Text>
                  </TouchableOpacity>
                )}
                {/* 兼容旧的 synced 字段，仅当启用同步且不在传输中时显示 */}
                {enableHistorySync &&
                  !isTransferring &&
                  item.syncStatus === undefined &&
                  item.synced !== undefined && (
                    <View style={styles.syncBadge}>
                      <Ionicons
                        name={item.synced ? 'cloud-done' : 'cloud-outline'}
                        size={14}
                        color={
                          item.synced
                            ? theme.colors.success || '#4CAF50'
                            : theme.colors.textTertiary
                        }
                      />
                      <Text
                        style={[
                          styles.syncBadgeText,
                          {
                            color: item.synced
                              ? theme.colors.success || '#4CAF50'
                              : theme.colors.textTertiary,
                          },
                        ]}
                      >
                        {item.synced ? '已同步' : '未同步'}
                      </Text>
                    </View>
                  )}
                {/* 未下载标识 - 图片/文件类型但本地文件未就绪，仅当启用同步且不在传输中时显示 */}
                {enableHistorySync &&
                  !isTransferring &&
                  (item.type === 'Image' || item.type === 'File') &&
                  item.isLocalFileReady === false && (
                    <TouchableOpacity
                      style={styles.syncBadge}
                      onPress={() => onDownload?.(item)}
                      disabled={!onDownload}
                    >
                      <Ionicons
                        name="cloud-download-outline"
                        size={14}
                        color={onDownload ? theme.colors.primary : theme.colors.textTertiary}
                      />
                      <Text
                        style={[
                          styles.syncBadgeText,
                          {
                            color: onDownload ? theme.colors.primary : theme.colors.textTertiary,
                          },
                        ]}
                      >
                        未下载
                      </Text>
                    </TouchableOpacity>
                  )}
              </View>
              <View style={styles.actionsRow}>
                {/* 收藏按钮 */}
                {onToggleStar && (
                  <TouchableOpacity
                    style={styles.actionButton}
                    onPress={() => onToggleStar(item)}
                    hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                  >
                    <StarIcon
                      size={ACTION_ICON_SIZE}
                      color={theme.colors.primary}
                      filled={!!item.starred}
                    />
                  </TouchableOpacity>
                )}
                {item.type === 'Text' && detectedUrl && (
                  <TouchableOpacity
                    style={styles.actionButton}
                    onPress={() => Linking.openURL(detectedUrl)}
                    hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                  >
                    <Link2
                      width={ACTION_ICON_SIZE}
                      height={ACTION_ICON_SIZE}
                      color={theme.colors.primary}
                    />
                  </TouchableOpacity>
                )}
                {item.type === 'Text' && onWordPick && item.text && (
                  <TouchableOpacity
                    style={styles.actionButton}
                    onPress={() => onWordPick(item.text!)}
                    hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                  >
                    <Scissors
                      width={ACTION_ICON_SIZE}
                      height={ACTION_ICON_SIZE}
                      color={theme.colors.primary}
                    />
                  </TouchableOpacity>
                )}
                {item.type === 'Text' && (
                  <TouchableOpacity
                    style={styles.actionButton}
                    onPress={() => onCopy(item)}
                    hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                  >
                    <Copy
                      width={ACTION_ICON_SIZE}
                      height={ACTION_ICON_SIZE}
                      color={theme.colors.primary}
                    />
                  </TouchableOpacity>
                )}
                {item.type === 'Image' && (
                  <>
                    {item.fileUri && onOpen && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onOpen(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <MaterialCommunityIcons
                          name="image-search-outline"
                          size={15}
                          color={theme.colors.primary}
                        />
                      </TouchableOpacity>
                    )}
                    {item.fileUri && onSave && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onSave(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <Download width={15} height={15} color={theme.colors.primary} />
                      </TouchableOpacity>
                    )}
                    {item.isLocalFileReady !== false && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onShare(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <Share width={15} height={15} color={theme.colors.primary} />
                      </TouchableOpacity>
                    )}
                    {item.fileUri && showImageCopyButton !== false && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onCopy(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <Copy
                          width={ACTION_ICON_SIZE}
                          height={ACTION_ICON_SIZE}
                          color={theme.colors.primary}
                        />
                      </TouchableOpacity>
                    )}
                  </>
                )}
                {(item.type === 'File' || item.type === 'Group') && (
                  <>
                    {item.fileUri && onOpen && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onOpen(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <MaterialCommunityIcons
                          name="file-eye-outline"
                          size={15}
                          color={theme.colors.primary}
                        />
                      </TouchableOpacity>
                    )}
                    {item.fileUri && onSave && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onSave(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <Download width={15} height={15} color={theme.colors.primary} />
                      </TouchableOpacity>
                    )}
                    {item.isLocalFileReady !== false && (
                      <TouchableOpacity
                        style={styles.actionButton}
                        onPress={() => onShare(item)}
                        hitSlop={{ top: 10, right: 10, bottom: 10, left: 10 }}
                      >
                        <Share width={15} height={15} color={theme.colors.primary} />
                      </TouchableOpacity>
                    )}
                  </>
                )}
              </View>
            </View>

            {/* 调试信息：profileHash */}
            {showDebugInfo && item.profileHash && (
              <View style={styles.debugRow}>
                <Text style={[styles.debugLabel, { color: theme.colors.textTertiary }]}>Hash:</Text>
                <Text style={[styles.debugValue, { color: theme.colors.textSecondary }]}>
                  {item.profileHash.substring(0, 16)}...
                </Text>
              </View>
            )}

            {/* 调试信息：fileUrl */}
            {showDebugInfo && item.fileUri && (
              <View style={styles.debugRow}>
                <Text style={[styles.debugLabel, { color: theme.colors.textTertiary }]}>URL:</Text>
                <Text
                  style={[
                    styles.debugValue,
                    styles.debugValueFlex,
                    { color: theme.colors.textSecondary },
                  ]}
                >
                  {item.fileUri}
                </Text>
              </View>
            )}

            {/* 调试信息：lastModified */}
            {showDebugInfo && item.lastModified !== undefined && (
              <View style={styles.debugRow}>
                <Text style={[styles.debugLabel, { color: theme.colors.textTertiary }]}>
                  LastModified:
                </Text>
                <Text style={[styles.debugValue, { color: theme.colors.textSecondary }]}>
                  {new Date(item.lastModified).toISOString()}
                </Text>
              </View>
            )}

            {/* 调试信息：lastAccessed */}
            {showDebugInfo && item.lastAccessed !== undefined && (
              <View style={styles.debugRow}>
                <Text style={[styles.debugLabel, { color: theme.colors.textTertiary }]}>
                  LastAccessed:
                </Text>
                <Text style={[styles.debugValue, { color: theme.colors.textSecondary }]}>
                  {new Date(item.lastAccessed).toISOString()}
                </Text>
              </View>
            )}

            {/* 调试信息：version */}
            {showDebugInfo && item.version !== undefined && (
              <View style={styles.debugRow}>
                <Text style={[styles.debugLabel, { color: theme.colors.textTertiary }]}>
                  Version:
                </Text>
                <Text style={[styles.debugValue, { color: theme.colors.textSecondary }]}>
                  {item.version}
                </Text>
              </View>
            )}
          </View>
        </View>
      </Pressable>
    );
  }
);

HistoryListItem.displayName = 'HistoryListItem';

const styles = StyleSheet.create({
  touchable: {
    marginHorizontal: spacing.base,
    marginVertical: spacing.xs,
    borderRadius: radius.lg,
    borderCurve: 'continuous',
  },
  touchableMultiSelect: {
    marginLeft: spacing.sm,
  },
  container: {
    flexDirection: 'column',
    padding: spacing.md,
    borderRadius: radius.lg,
    borderCurve: 'continuous',
  },
  containerMultiSelect: {
    flex: 1,
    minWidth: 0,
  },
  containerSelected: {
    borderWidth: 2,
  },
  multiSelectRow: {
    flexDirection: 'row',
    alignItems: 'flex-start',
  },
  checkboxContainer: {
    justifyContent: 'center',
    alignItems: 'center',
    marginRight: spacing.sm,
    paddingTop: spacing.md,
  },
  topContent: {
    flexDirection: 'row',
    alignItems: 'center',
    marginBottom: spacing.sm,
  },
  bottomContent: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginTop: spacing.sm,
  },
  iconContainer: {
    width: 28,
    height: 28,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: radius.pill,
    overflow: 'hidden',
  },
  typeIcon: {
    fontSize: 14,
  },
  imagePreviewContainer: {
    marginTop: spacing.sm,
    marginBottom: spacing.xs,
    borderRadius: radius.md,
    borderCurve: 'continuous',
    overflow: 'hidden',
    alignSelf: 'stretch',
  },
  imagePreview: {
    width: '100%',
  },
  imagePreviewLimited: {
    height: 180,
  },
  contentContainer: {
    flex: 1,
    marginLeft: spacing.sm,
  },
  headerRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: spacing.xs,
  },
  typeLabel: {
    fontSize: typography.footnote.fontSize,
    fontWeight: '600',
  },
  typeLabelSpacing: {
    marginLeft: spacing.sm,
    marginRight: spacing.sm,
  },
  timestamp: {
    fontSize: typography.caption1.fontSize,
  },
  timestampAlign: {
    flex: 1,
    textAlign: 'right',
  },
  previewText: {
    fontSize: typography.subhead.fontSize,
    lineHeight: typography.subhead.lineHeight,
    marginBottom: spacing.xs,
  },

  metaInfo: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
  },
  metaText: {
    fontSize: typography.caption1.fontSize,
  },
  syncBadge: {
    marginLeft: spacing.xs,
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.xs,
  },
  syncBadgeText: {
    fontSize: typography.caption1.fontSize,
    fontWeight: '500',
  },
  actionsRow: {
    flexDirection: 'row',
    gap: 6,
  },
  actionButton: {
    width: 28,
    height: 28,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: radius.pill,
  },
  progressContainer: {
    width: 28,
    height: 28,
    alignItems: 'center',
    justifyContent: 'center',
  },
  progressOverlay: {
    position: 'absolute',
    bottom: -4,
  },
  progressText: {
    fontSize: 8,
    fontWeight: '600',
  },
  errorText: {
    fontSize: 10,
    marginLeft: 4,
    flex: 1,
  },
  actionButtonIcon: {
    fontSize: ACTION_ICON_SIZE,
  },
  debugRow: {
    flexDirection: 'row',
    alignItems: 'center',
    marginTop: 4,
    gap: 4,
  },
  debugLabel: {
    fontSize: 11,
    fontWeight: '500',
  },
  debugValue: {
    fontSize: 11,
    fontFamily: 'monospace',
  },
  debugValueFlex: {
    flex: 1,
  },
});
