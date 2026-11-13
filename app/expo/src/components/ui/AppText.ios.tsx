import { Text } from '@expo/ui/swift-ui';
import { font, foregroundStyle } from '@expo/ui/swift-ui/modifiers';
import type { ColorValue } from 'react-native';

export interface AppTextProps {
  children: string;
  color?: ColorValue;
  typography?: string;
}

// Approximate Material 3 typography sizes (sp) so iOS text scales similarly.
// TODO: replace with SwiftUI Font.TextStyle mapping.
const TYPOGRAPHY_SIZE: Record<string, { size: number; weight?: string }> = {
  displayLarge: { size: 57 },
  displayMedium: { size: 45 },
  displaySmall: { size: 36 },
  headlineLarge: { size: 32 },
  headlineMedium: { size: 28 },
  headlineSmall: { size: 24 },
  titleLarge: { size: 22, weight: 'medium' },
  titleMedium: { size: 16, weight: 'medium' },
  titleSmall: { size: 14, weight: 'medium' },
  bodyLarge: { size: 16 },
  bodyMedium: { size: 14 },
  bodySmall: { size: 12 },
  labelLarge: { size: 14, weight: 'medium' },
  labelMedium: { size: 12, weight: 'medium' },
  labelSmall: { size: 11, weight: 'medium' },
};

export function AppText({ children, color, typography }: AppTextProps) {
  const spec = typography ? TYPOGRAPHY_SIZE[typography] : undefined;
  const modifiers = [
    ...(spec ? [font({ size: spec.size, weight: spec.weight as never })] : []),
    ...(color ? [foregroundStyle(color as string)] : []),
  ];
  return <Text modifiers={modifiers}>{children}</Text>;
}
