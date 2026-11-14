import {
  buttonStyle,
  foregroundStyle,
  frame,
  tint,
  type ModifierConfig,
} from '@expo/ui/swift-ui/modifiers';
import type { ColorValue } from 'react-native';

import {
  iosAccent,
  iosAccentColor,
  iosColors,
  iosOnAccent,
  iosOnAccentColor,
  iosOnSaturatedColor,
  iosSystemHex,
} from '@/theme/iosDesignTokens';

export interface IosProminentButtonPalette {
  background: ColorValue;
  foreground: ColorValue;
}

interface IosButtonLayoutOptions {
  fullWidth?: boolean;
}

export const iosAccentButtonPalette: IosProminentButtonPalette = {
  background: iosAccentColor ?? iosAccent.light,
  foreground: iosOnAccentColor ?? iosOnAccent.light,
};

export function iosSaturatedButtonPalette(background: ColorValue): IosProminentButtonPalette {
  return { background, foreground: iosOnSaturatedColor };
}

export function iosProminentButtonModifiers(
  palette: IosProminentButtonPalette = iosAccentButtonPalette,
  { fullWidth = false }: IosButtonLayoutOptions = {}
): ModifierConfig[] {
  return [
    buttonStyle('borderedProminent'),
    tint(palette.background),
    foregroundStyle(palette.foreground),
    ...(fullWidth ? [frame({ maxWidth: Infinity })] : []),
  ];
}

export function iosSecondaryButtonModifiers({
  fullWidth = false,
}: IosButtonLayoutOptions = {}): ModifierConfig[] {
  const foreground = iosColors?.label ?? iosSystemHex.label.light;
  return [
    buttonStyle('bordered'),
    tint(foreground),
    foregroundStyle(foreground),
    ...(fullWidth ? [frame({ maxWidth: Infinity })] : []),
  ];
}
