import React from 'react';
import { View, Pressable, StyleSheet } from 'react-native';
import { useTranslation } from 'react-i18next';
import { Copy, Send, Share, Trash2, type LucideIcon } from 'lucide-react-native';
import { GlassContainer } from '@/components/ui';
import { iosDimensions } from '@/theme/iosDesignTokens';
import type {
  SelectModeBottomBarContainerProps,
  SelectModeBottomBarProps,
} from './HomeBottomBar.types';

const BTN = iosDimensions.floatingButtonSize;
const DESTRUCTIVE = '#D70015';

function GlassAction({
  testID,
  icon: Icon,
  label,
  color,
  disabled,
  onPress,
}: {
  testID: string;
  icon: LucideIcon;
  label: string;
  color: SelectModeBottomBarProps['theme']['colors']['textPrimary'];
  disabled: boolean;
  onPress: () => void;
}) {
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled }}
    >
      <GlassContainer shape="circle" interactive style={s.circle}>
        <Icon size={22} color={color} />
      </GlassContainer>
    </Pressable>
  );
}

/** 多选底栏:一排独立的 Liquid Glass 圆钮(复制 / 分享 / 发送到 / 删除),未选中时置灰。 */
export function SelectModeBottomBar({
  disabled,
  onCopy,
  onShare,
  onSendTo,
  onDelete,
  theme,
}: SelectModeBottomBarProps) {
  const { t } = useTranslation('common');
  const tint = disabled ? theme.colors.textTertiary : theme.colors.textPrimary;
  return (
    <View style={s.selectRow}>
      <GlassAction
        testID="history-batch-copy"
        icon={Copy}
        label={t('action.copy')}
        color={tint}
        disabled={disabled}
        onPress={onCopy}
      />
      <GlassAction
        testID="history-batch-share"
        icon={Share}
        label={t('action.share')}
        color={tint}
        disabled={disabled}
        onPress={onShare}
      />
      {onSendTo ? (
        <GlassAction
          testID="history-batch-send"
          icon={Send}
          label={t('detail.sendTo', { ns: 'home' })}
          color={tint}
          disabled={disabled}
          onPress={onSendTo}
        />
      ) : null}
      <GlassAction
        testID="history-batch-delete"
        icon={Trash2}
        label={t('action.delete')}
        color={disabled ? theme.colors.textTertiary : DESTRUCTIVE}
        disabled={disabled}
        onPress={onDelete}
      />
    </View>
  );
}

/** 单栏首页的多选底栏容器:浮在内容之上,没有底板 */
export function SelectModeBottomBarContainer({ bottomInset, children }: SelectModeBottomBarContainerProps) {
  return (
    <View pointerEvents="box-none" style={[s.container, { bottom: Math.max(16, bottomInset) }]}>
      {children}
    </View>
  );
}

const s = StyleSheet.create({
  container: { position: 'absolute', left: 0, right: 0 },
  circle: { width: BTN, height: BTN, justifyContent: 'center', alignItems: 'center' },
  selectRow: { flexDirection: 'row', justifyContent: 'center', gap: 20 },
});
