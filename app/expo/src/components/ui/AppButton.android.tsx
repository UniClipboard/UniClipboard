import {
  Button,
  OutlinedButton,
  FilledTonalButton,
  TextButton,
  Text as ComposeText,
} from '@expo/ui/jetpack-compose';
import { defaultMinSize, fillMaxWidth, testID as testIDModifier } from '@expo/ui/jetpack-compose/modifiers';

import type { AppButtonProps } from './AppButton.types';

export type { AppButtonProps, AppButtonVariant } from './AppButton.types';

const VARIANT_MAP = {
  filled: Button,
  outlined: OutlinedButton,
  tonal: FilledTonalButton,
  text: TextButton,
} as const;

const LARGE_LABEL_STYLE = { fontSize: 16 };

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
  const Component = VARIANT_MAP[variant];
  const modifiers = [
    ...(testID ? [testIDModifier(testID)] : []),
    ...(fullWidth ? [fillMaxWidth()] : []),
    ...(size === 'large' ? [defaultMinSize({ minHeight: 56 })] : []),
  ];
  return (
    <Component
      onClick={onPress}
      enabled={disabled !== undefined ? !disabled : undefined}
      colors={colors}
      modifiers={modifiers.length ? modifiers : undefined}
    >
      <ComposeText style={size === 'large' ? LARGE_LABEL_STYLE : undefined}>{title}</ComposeText>
    </Component>
  );
}
