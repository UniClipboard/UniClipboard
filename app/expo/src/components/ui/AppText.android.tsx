import { Text as ComposeText, type TextProps } from '@expo/ui/jetpack-compose';
import type { ColorValue } from 'react-native';

type TypographyStyle = NonNullable<NonNullable<TextProps['style']>['typography']>;

export interface AppTextProps {
  children: string;
  color?: ColorValue;
  typography?: TypographyStyle;
}

export function AppText({ children, color, typography }: AppTextProps) {
  return (
    <ComposeText
      color={color as string | undefined}
      style={typography ? { typography } : undefined}
    >
      {children}
    </ComposeText>
  );
}
