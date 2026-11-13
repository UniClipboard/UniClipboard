import { View, StyleSheet, type ViewStyle } from 'react-native';

export interface AppRowProps {
  children: React.ReactNode;
  align?: 'top' | 'center' | 'bottom';
  justify?: 'start' | 'end' | 'center' | 'spaceBetween' | 'spaceAround' | 'spaceEvenly';
  spacing?: number;
  fullWidth?: boolean;
  padding?: number;
}

const ALIGN_MAP: Record<string, ViewStyle['alignItems']> = {
  top: 'flex-start',
  center: 'center',
  bottom: 'flex-end',
};

const JUSTIFY_MAP: Record<string, ViewStyle['justifyContent']> = {
  start: 'flex-start',
  end: 'flex-end',
  center: 'center',
  spaceBetween: 'space-between',
  spaceAround: 'space-around',
  spaceEvenly: 'space-evenly',
};

export function AppRow({ children, align, justify, spacing, fullWidth, padding }: AppRowProps) {
  return (
    <View
      style={[
        styles.base,
        align ? { alignItems: ALIGN_MAP[align] } : null,
        justify ? { justifyContent: JUSTIFY_MAP[justify] } : null,
        spacing != null ? { gap: spacing } : null,
        fullWidth ? styles.fullWidth : null,
        padding != null ? { padding } : null,
      ]}
    >
      {children}
    </View>
  );
}

const styles = StyleSheet.create({
  base: {
    flexDirection: 'row',
  },
  fullWidth: {
    alignSelf: 'stretch',
  },
});
