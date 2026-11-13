import type React from 'react';
import type { useTheme } from '@/hooks/useTheme';

export interface SelectModeBottomBarProps {
  disabled: boolean;
  onCopy: () => void;
  onShare: () => void;
  /** 发送到其他设备(iOS 多选底栏);不传则不显示 */
  onSendTo?: () => void;
  onDelete: () => void;
  theme: ReturnType<typeof useTheme>['theme'];
}

export interface SelectModeBottomBarContainerProps {
  bottomInset: number;
  theme: ReturnType<typeof useTheme>['theme'];
  children: React.ReactNode;
}
