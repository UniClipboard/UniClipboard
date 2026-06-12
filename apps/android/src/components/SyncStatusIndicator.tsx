/**
 * Sync Status Indicator Component
 * 同步状态指示器
 */

import React from 'react';
import { View, Text, StyleSheet, ActivityIndicator } from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { spacing, typography } from '@/theme';
import { SyncStatus } from '@/types/sync';

interface SyncStatusIndicatorProps {
  status: SyncStatus;
  lastSyncTime: number | null;
  serverConnected: boolean;
}

export const SyncStatusIndicator: React.FC<SyncStatusIndicatorProps> = ({
  status,
  lastSyncTime,
  serverConnected,
}) => {
  const { theme } = useTheme();

  const getStatusColor = (): string => {
    if (!serverConnected) return theme.colors.textTertiary;

    switch (status) {
      case SyncStatus.Syncing:
        return theme.colors.primary;
      case SyncStatus.Success:
        return '#4CAF50'; // Green
      case SyncStatus.Failed:
        return '#F44336'; // Red
      case SyncStatus.Conflict:
        return '#FF9800'; // Orange
      case SyncStatus.Idle:
      default:
        return theme.colors.textSecondary;
    }
  };

  const getStatusIcon = (): string => {
    if (!serverConnected) return '⚠️';

    switch (status) {
      case SyncStatus.Syncing:
        return '🔄';
      case SyncStatus.Success:
        return '✅';
      case SyncStatus.Failed:
        return '❌';
      case SyncStatus.Conflict:
        return '⚠️';
      case SyncStatus.Idle:
      default:
        return '⏸️';
    }
  };

  const getStatusText = (): string => {
    if (!serverConnected) return '未连接服务器';

    switch (status) {
      case SyncStatus.Syncing:
        return '同步中...';
      case SyncStatus.Success:
        return '已同步';
      case SyncStatus.Failed:
        return '同步失败';
      case SyncStatus.Conflict:
        return '同步冲突';
      case SyncStatus.Idle:
      default:
        return '等待同步';
    }
  };

  const formatLastSyncTime = (): string => {
    if (!lastSyncTime) return '';

    const now = Date.now();
    const diff = now - lastSyncTime;

    if (diff < 60000) return '刚刚同步';
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;

    return new Date(lastSyncTime).toLocaleDateString('zh-CN', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const statusColor = getStatusColor();
  const statusIcon = getStatusIcon();
  const statusText = getStatusText();
  const timeText = formatLastSyncTime();

  return (
    <View style={[styles.container, { backgroundColor: theme.colors.surface }]}>
      <View style={styles.content}>
        <View style={styles.statusRow}>
          {status === SyncStatus.Syncing ? (
            <ActivityIndicator size="small" color={statusColor} style={styles.icon} />
          ) : (
            <Text style={styles.iconText}>{statusIcon}</Text>
          )}

          <View style={styles.textContainer}>
            <Text style={[styles.statusText, { color: statusColor }]}>{statusText}</Text>
            {timeText && (
              <Text style={[styles.timeText, { color: theme.colors.onSurfaceVariant }]}>
                {timeText}
              </Text>
            )}
          </View>
        </View>

        {/* 服务器状态指示点 */}
        <View style={styles.connectionDot}>
          <View
            style={[
              styles.dot,
              {
                backgroundColor: serverConnected ? theme.colors.success : theme.colors.outline,
              },
            ]}
          />
        </View>
      </View>
    </View>
  );
};

const styles = StyleSheet.create({
  container: {
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  content: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  statusRow: {
    flexDirection: 'row',
    alignItems: 'center',
    flex: 1,
  },
  icon: {
    marginRight: spacing.sm,
  },
  iconText: {
    fontSize: 20,
    marginRight: spacing.sm,
  },
  textContainer: {
    flex: 1,
  },
  statusText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
  },
  timeText: {
    fontSize: typography.caption1.fontSize,
    marginTop: 2,
  },
  connectionDot: {
    marginLeft: spacing.md,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
});
