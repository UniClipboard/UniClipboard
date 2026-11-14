/**
 * 分组列表行的前导图标:40dp 圆形色块 + 22dp 图标(M3 Expressive list 的 leading avatar)。
 * 设备、服务器、空间设置等 grouped 分组行共用;色调由调用方按语义(常规 / 静默 / 错误 /
 * 强调)选择,形状与尺寸在此统一。
 */
import { memo } from 'react';
import { Box, Icon, Shape, Surface, useMaterialColors } from '@expo/ui/jetpack-compose';
import { size } from '@expo/ui/jetpack-compose/modifiers';

export type SettingsLeadingIconTone = 'primary' | 'muted' | 'error' | 'accent';

const CIRCLE_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 20, topEnd: 20, bottomStart: 20, bottomEnd: 20 },
});

export const SettingsLeadingIcon = memo(function SettingsLeadingIcon({
  source,
  tone = 'primary',
}: {
  source: number;
  tone?: SettingsLeadingIconTone;
}) {
  const colors = useMaterialColors();
  const [container, content] =
    tone === 'muted'
      ? [colors.surfaceContainerHighest, colors.onSurfaceVariant]
      : tone === 'error'
      ? [colors.errorContainer, colors.error]
      : tone === 'accent'
      ? [colors.primary, colors.onPrimary]
      : [colors.primaryContainer, colors.onPrimaryContainer];

  return (
    <Surface color={container} shape={CIRCLE_SHAPE} modifiers={[size(40, 40)]}>
      <Box contentAlignment="center" modifiers={[size(40, 40)]}>
        <Icon source={source} size={22} tint={content} />
      </Box>
    </Surface>
  );
});
