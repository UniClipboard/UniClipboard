import { View, StyleSheet, PlatformColor, type ColorValue } from 'react-native';

export interface AppDividerProps {
  color?: ColorValue;
  thickness?: number;
}

const defaultColor = PlatformColor('separator');

export function AppDivider({ color, thickness }: AppDividerProps) {
  return (
    <View
      style={{
        height: thickness ?? StyleSheet.hairlineWidth,
        alignSelf: 'stretch',
        backgroundColor: color ?? defaultColor,
      }}
    />
  );
}
