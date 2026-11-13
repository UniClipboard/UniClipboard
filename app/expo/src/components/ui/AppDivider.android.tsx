import { HorizontalDivider } from '@expo/ui/jetpack-compose';
import type { ColorValue } from 'react-native';

export interface AppDividerProps {
  color?: ColorValue;
  thickness?: number;
}

export function AppDivider({ color, thickness }: AppDividerProps) {
  return <HorizontalDivider color={color} thickness={thickness} />;
}
