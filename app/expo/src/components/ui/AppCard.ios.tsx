import { View, StyleSheet, DynamicColorIOS, type ColorValue } from 'react-native';

export interface AppCardProps {
  children: React.ReactNode;
  containerColor?: ColorValue;
  elevation?: number;
  fullWidth?: boolean;
}

const defaultCardBg = DynamicColorIOS({ light: '#FFFFFF', dark: '#1C1C1E' });

export function AppCard({ children, containerColor, elevation = 1, fullWidth }: AppCardProps) {
  return (
    <View
      style={[
        styles.card,
        { backgroundColor: containerColor ?? defaultCardBg },
        { shadowOpacity: 0.1 + Math.min(elevation, 6) * 0.02, shadowRadius: elevation },
        fullWidth ? styles.fullWidth : null,
      ]}
    >
      {children}
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    borderRadius: 12,
    padding: 16,
    shadowColor: '#000000',
    shadowOffset: { width: 0, height: 1 },
  },
  fullWidth: {
    alignSelf: 'stretch',
  },
});
