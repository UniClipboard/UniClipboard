import type { ColorValue } from 'react-native';

export type AppButtonVariant = 'filled' | 'outlined' | 'tonal' | 'text';

export interface AppButtonProps {
  title: string;
  testID?: string;
  onPress: () => void;
  variant?: AppButtonVariant;
  fullWidth?: boolean;
  size?: 'regular' | 'large';
  disabled?: boolean;
  colors?: {
    containerColor?: ColorValue;
    contentColor?: ColorValue;
  };
}
