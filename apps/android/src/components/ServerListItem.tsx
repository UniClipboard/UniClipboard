/**
 * 服务器列表项组件
 * 显示单个服务器配置信息
 */

import React from 'react';
import { View, Text, StyleSheet, TouchableOpacity, Alert } from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography, alpha } from '@/theme';
import { ServerConfig } from '@/types/api';

interface ServerListItemProps {
  config: ServerConfig;
  isActive: boolean;
  onPress: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export const ServerListItem: React.FC<ServerListItemProps> = ({
  config,
  isActive,
  onPress,
  onEdit,
  onDelete,
}) => {
  const { theme } = useTheme();

  const handleDelete = () => {
    Alert.alert('确认删除', `确定要删除服务器 "${getServerDisplayName(config)}" 吗？`, [
      { text: '取消', style: 'cancel' },
      { text: '删除', style: 'destructive', onPress: onDelete },
    ]);
  };

  const getServerDisplayName = (config: ServerConfig): string => {
    if (config.name) return config.name;
    try {
      const url = new URL(config.url);
      return url.hostname;
    } catch {
      return config.url;
    }
  };

  const getServerTypeLabel = (type: string): string => {
    switch (type) {
      case 'syncclipboard':
        return 'SyncClipboard';
      case 's3':
        return 'S3';
      default:
        return 'WebDAV';
    }
  };

  const getTypeBadgeColors = (type: string) => {
    if (type === 'syncclipboard') {
      return {
        backgroundColor: alpha('#4CAF50', 0.16),
        color: theme.isDark ? '#A5D6A7' : '#1B5E20',
      };
    } else if (type === 's3') {
      return {
        backgroundColor: alpha('#2196F3', 0.16),
        color: theme.isDark ? '#90CAF9' : '#0D47A1',
      };
    } else {
      return {
        backgroundColor: alpha('#FF9800', 0.16),
        color: theme.isDark ? '#FFD58A' : '#8C5400',
      };
    }
  };

  return (
    <TouchableOpacity
      style={[
        styles.container,
        {
          backgroundColor: isActive
            ? theme.colors.primaryContainer
            : theme.colors.surfaceContainerLow,
        },
        isActive && { borderWidth: 1.5, borderColor: theme.colors.primary },
      ]}
      onPress={onPress}
      activeOpacity={0.7}
    >
      {/* 服务器信息 */}
      <View style={styles.content}>
        <View style={styles.mainInfo}>
          <Text style={[styles.serverName, { color: theme.colors.text }]} numberOfLines={1}>
            {getServerDisplayName(config)}
          </Text>
          <View
            style={[
              styles.typeBadge,
              { backgroundColor: getTypeBadgeColors(config.type).backgroundColor },
            ]}
          >
            <Text style={[styles.typeText, { color: getTypeBadgeColors(config.type).color }]}>
              {getServerTypeLabel(config.type)}
            </Text>
          </View>
        </View>

        {config.type === 's3' && config.region ? (
          <Text style={[styles.serverUrl, { color: theme.colors.textSecondary }]} numberOfLines={1}>
            🌍 {config.region}
          </Text>
        ) : (
          <Text style={[styles.serverUrl, { color: theme.colors.textSecondary }]} numberOfLines={1}>
            {config.url}
          </Text>
        )}

        <View style={styles.details}>
          <Text style={[styles.detailText, { color: theme.colors.textSecondary }]}>
            {config.type === 's3'
              ? `🪣 ${config.bucketName || '未设置'}`
              : `👤 ${config.username || '未设置'}`}
          </Text>
        </View>
      </View>

      {/* 操作按钮 — M3 Filled Tonal */}
      <View style={styles.actions}>
        <TouchableOpacity
          style={[styles.actionButton, { backgroundColor: theme.colors.primaryContainer }]}
          onPress={(e) => {
            e.stopPropagation();
            onEdit();
          }}
        >
          <Text style={[styles.actionButtonText, { color: theme.colors.onPrimaryContainer }]}>
            编辑
          </Text>
        </TouchableOpacity>

        <TouchableOpacity
          style={[styles.actionButton, { backgroundColor: theme.colors.errorContainer }]}
          onPress={(e) => {
            e.stopPropagation();
            handleDelete();
          }}
        >
          <Text style={[styles.actionButtonText, { color: theme.colors.onErrorContainer }]}>
            删除
          </Text>
        </TouchableOpacity>
      </View>
    </TouchableOpacity>
  );
};

const styles = StyleSheet.create({
  container: {
    borderRadius: radius.lg,
    borderCurve: 'continuous',
    padding: spacing.base,
    marginHorizontal: spacing.base,
    marginBottom: spacing.md,
  },
  content: {
    marginBottom: spacing.md,
  },
  mainInfo: {
    flexDirection: 'row',
    alignItems: 'center',
    marginBottom: spacing.sm,
  },
  serverName: {
    fontSize: typography.headline.fontSize,
    fontWeight: typography.headline.fontWeight,
    flex: 1,
    marginRight: spacing.sm,
  },
  typeBadge: {
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.xs,
    borderRadius: radius.sm,
  },
  typeText: {
    fontSize: typography.caption2.fontSize,
    fontWeight: '600',
  },
  serverUrl: {
    fontSize: typography.footnote.fontSize,
    marginBottom: spacing.sm,
  },
  details: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.base,
  },
  detailText: {
    fontSize: typography.footnote.fontSize,
  },
  actions: {
    flexDirection: 'row',
    gap: spacing.sm,
  },
  actionButton: {
    flex: 1,
    paddingVertical: spacing.sm,
    paddingHorizontal: spacing.md,
    borderRadius: radius.pill,
    alignItems: 'center',
  },
  actionButtonText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
  },
});
