import { View, StyleSheet, type ViewStyle } from 'react-native';

export interface AppColumnProps {
  children: React.ReactNode;
  align?: 'start' | 'center' | 'end';
  justify?: 'top' | 'bottom' | 'center' | 'spaceBetween' | 'spaceAround' | 'spaceEvenly';
  spacing?: number;
  fullWidth?: boolean;
  padding?: number;
}

const ALIGN_MAP: Record<string, ViewStyle['alignItems']> = {
  start: 'flex-start',
  center: 'center',
  end: 'flex-end',
};

const JUSTIFY_MAP: Record<string, ViewStyle['justifyContent']> = {
  top: 'flex-start',
  bottom: 'flex-end',
  center: 'center',
  spaceBetween: 'space-between',
  spaceAround: 'space-around',
  spaceEvenly: 'space-evenly',
};

export function AppColumn({
  children,
  align,
  justify,
  spacing,
  fullWidth,
  padding,
}: AppColumnProps) {
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
    flexDirection: 'column',
  },
  fullWidth: {
    alignSelf: 'stretch',
  },
});
