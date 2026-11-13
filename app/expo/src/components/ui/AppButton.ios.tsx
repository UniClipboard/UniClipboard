import { Button, HStack, Spacer, Text } from '@expo/ui/swift-ui';
import {
  buttonBorderShape,
  buttonStyle,
  controlSize,
  disabled as disabledModifier,
  frame,
  foregroundStyle,
  tint,
} from '@expo/ui/swift-ui/modifiers';

import { iosOnSaturatedColor } from '@/theme/iosDesignTokens';
import {
  iosAccentButtonPalette,
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from './iosButtonStyles.ios';
import type { AppButtonProps } from './AppButton.types';

export type { AppButtonProps, AppButtonVariant } from './AppButton.types';

export function AppButton({
  title,
  testID,
  onPress,
  variant = 'filled',
  fullWidth,
  size = 'regular',
  disabled,
  colors,
}: AppButtonProps) {
  const filledPalette = colors?.containerColor
    ? {
        background: colors.containerColor,
        foreground: colors.contentColor ?? iosOnSaturatedColor,
      }
    : {
        ...iosAccentButtonPalette,
        ...(colors?.contentColor ? { foreground: colors.contentColor } : {}),
      };
  const variantModifiers =
    variant === 'filled'
      ? iosProminentButtonModifiers(filledPalette, { fullWidth })
      : variant === 'outlined' || variant === 'tonal'
      ? [
          ...iosSecondaryButtonModifiers({ fullWidth }),
          ...(colors?.containerColor ? [tint(colors.containerColor)] : []),
          ...(colors?.contentColor ? [foregroundStyle(colors.contentColor)] : []),
        ]
      : [
          buttonStyle('plain'),
          ...(fullWidth ? [frame({ maxWidth: Infinity })] : []),
          ...(colors?.contentColor ? [foregroundStyle(colors.contentColor)] : []),
        ];
  const modifiers = [
    ...variantModifiers,
    controlSize(size),
    ...(size === 'large' ? [buttonBorderShape('capsule')] : []),
    ...(disabled ? [disabledModifier(true)] : []),
  ];

  if (fullWidth && size === 'large') {
    return (
      <Button testID={testID} onPress={onPress} modifiers={modifiers}>
        <HStack modifiers={[frame({ maxWidth: Infinity, minHeight: 50 })]}>
          <Spacer />
          <Text>{title}</Text>
          <Spacer />
        </HStack>
      </Button>
    );
  }

  return <Button testID={testID} label={title} onPress={onPress} modifiers={modifiers} />;
}
