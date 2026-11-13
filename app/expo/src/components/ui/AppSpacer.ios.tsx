import { View } from 'react-native';

export interface AppSpacerProps {
  /** When set, the spacer takes a fixed size (points) instead of flexing. */
  size?: number;
  /** Axis for the fixed size. @default 'vertical' */
  axis?: 'vertical' | 'horizontal';
}

export function AppSpacer({ size, axis = 'vertical' }: AppSpacerProps) {
  if (size != null) {
    return <View style={axis === 'horizontal' ? { width: size } : { height: size }} />;
  }
  return <View style={{ flex: 1 }} />;
}
