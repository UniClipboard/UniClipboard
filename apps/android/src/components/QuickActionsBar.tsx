/**
 * Quick Actions Bar Component
 * 快速操作栏 - 底部悬浮操作按钮
 */

import React from 'react';
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  ActivityIndicator,
  Platform,
} from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography } from '@/theme';

interface QuickActionsBarProps {
  onUpload: () => void;
  onDownload: () => void;
  onSync: () => void;
  disabled?: boolean;
  syncInProgress?: boolean;
}

export const QuickActionsBar: React.FC<QuickActionsBarProps> = ({
  onUpload,
  onDownload,
  onSync,
  disabled = false,
  syncInProgress = false,
}) => {
  const { theme } = useTheme();

  return (
    <View style={[styles.container, { backgroundColor: theme.colors.surface }]}>
      {/* 上传按钮 — Filled Tonal */}
      <TouchableOpacity
        style={[
          styles.button,
          disabled && styles.buttonDisabled,
          { backgroundColor: theme.colors.surfaceContainerHigh },
        ]}
        onPress={onUpload}
        disabled={disabled || syncInProgress}
        activeOpacity={0.7}
      >
        <Text style={styles.buttonIcon}>⬆️</Text>
        <Text
          style={[
            styles.buttonText,
            { color: disabled ? theme.colors.outline : theme.colors.onSurface },
          ]}
        >
          上传
        </Text>
      </TouchableOpacity>

      {/* 同步按钮 (主操作) — M3 Filled Tonal 主色 */}
      <TouchableOpacity
        style={[
          styles.syncButton,
          disabled && styles.buttonDisabled,
          {
            backgroundColor: disabled
              ? theme.colors.surfaceContainerHigh
              : theme.colors.primaryContainer,
          },
        ]}
        onPress={onSync}
        disabled={disabled || syncInProgress}
        activeOpacity={0.7}
      >
        {syncInProgress ? (
          <ActivityIndicator size="small" color={theme.colors.onPrimaryContainer} />
        ) : (
          <Text style={styles.syncButtonIcon}>🔄</Text>
        )}
        <Text style={[styles.syncButtonText, { color: theme.colors.onPrimaryContainer }]}>
          {syncInProgress ? '同步中...' : '同步'}
        </Text>
      </TouchableOpacity>

      {/* 下载按钮 — Filled Tonal */}
      <TouchableOpacity
        style={[
          styles.button,
          disabled && styles.buttonDisabled,
          { backgroundColor: theme.colors.surfaceContainerHigh },
        ]}
        onPress={onDownload}
        disabled={disabled || syncInProgress}
        activeOpacity={0.7}
      >
        <Text style={styles.buttonIcon}>⬇️</Text>
        <Text
          style={[
            styles.buttonText,
            { color: disabled ? theme.colors.outline : theme.colors.onSurface },
          ]}
        >
          下载
        </Text>
      </TouchableOpacity>
    </View>
  );
};

const styles = StyleSheet.create({
  container: {
    flexDirection: 'row',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
    paddingBottom: Platform.OS === 'ios' ? spacing.xl : spacing.md,
  },
  button: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingVertical: spacing.md,
    borderRadius: radius.pill,
    marginHorizontal: spacing.xs,
  },
  buttonDisabled: {
    opacity: 0.5,
  },
  buttonIcon: {
    fontSize: 20,
    marginRight: 6,
  },
  buttonText: {
    fontSize: typography.subhead.fontSize,
    fontWeight: '600',
  },
  syncButton: {
    flex: 1.5,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingVertical: spacing.md + 2,
    borderRadius: radius.pill,
    marginHorizontal: spacing.sm,
  },
  syncButtonIcon: {
    fontSize: 22,
    marginRight: spacing.sm,
  },
  syncButtonText: {
    fontSize: typography.callout.fontSize,
    fontWeight: '700',
  },
});
