import { AlertDialog, Host, TextButton, Text as ComposeText } from '@expo/ui/jetpack-compose';
import type { ColorValue } from 'react-native';
import { useTheme } from '@/hooks/useTheme';
import { MATERIAL_SEED_COLOR } from '@/theme/colors';

export interface AppAlertDialogProps {
  visible: boolean;
  standalone?: boolean;
  onDismiss: () => void;
  title: string;
  message?: string;
  confirmLabel: string;
  onConfirm: () => void;
  dismissLabel?: string;
  onDismissAction?: () => void;
  containerColor?: ColorValue;
}

export function AppAlertDialog({
  visible,
  standalone = false,
  onDismiss,
  title,
  message,
  confirmLabel,
  onConfirm,
  dismissLabel,
  onDismissAction,
  containerColor,
}: AppAlertDialogProps) {
  const { theme } = useTheme();
  if (!visible) return null;
  const dialog = (
    <AlertDialog
      onDismissRequest={onDismiss}
      colors={containerColor ? { containerColor } : undefined}
    >
      <AlertDialog.Title>
        <ComposeText>{title}</ComposeText>
      </AlertDialog.Title>
      {message ? (
        <AlertDialog.Text>
          <ComposeText>{message}</ComposeText>
        </AlertDialog.Text>
      ) : null}
      <AlertDialog.ConfirmButton>
        <TextButton onClick={onConfirm}>
          <ComposeText>{confirmLabel}</ComposeText>
        </TextButton>
      </AlertDialog.ConfirmButton>
      {dismissLabel ? (
        <AlertDialog.DismissButton>
          <TextButton onClick={onDismissAction ?? onDismiss}>
            <ComposeText>{dismissLabel}</ComposeText>
          </TextButton>
        </AlertDialog.DismissButton>
      ) : null}
    </AlertDialog>
  );
  return standalone ? (
    <Host colorScheme={theme.isDark ? 'dark' : 'light'} seedColor={MATERIAL_SEED_COLOR}>
      {dialog}
    </Host>
  ) : dialog;
}
