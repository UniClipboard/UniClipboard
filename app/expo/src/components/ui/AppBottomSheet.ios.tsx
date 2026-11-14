import {
  Modal,
  View,
  Pressable,
  StyleSheet,
  DynamicColorIOS,
  type ColorValue,
  useColorScheme,
} from 'react-native';
import { BlurView } from 'expo-blur';

export interface AppBottomSheetProps {
  visible: boolean;
  onDismiss: () => void;
  children: React.ReactNode;
  containerColor?: ColorValue;
}

export function AppBottomSheet({
  visible,
  onDismiss,
  children,
  containerColor,
}: AppBottomSheetProps) {
  const colorScheme = useColorScheme();
  const isDark = colorScheme === 'dark';

  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onDismiss}>
      <Pressable style={styles.backdrop} onPress={onDismiss}>
        <Pressable style={styles.sheet} onPress={() => {}}>
          <BlurView
            intensity={90}
            tint={isDark ? 'systemChromeMaterialDark' : 'systemChromeMaterial'}
            style={[
              StyleSheet.absoluteFill,
              styles.sheetBlur,
              containerColor ? { backgroundColor: containerColor } : null,
            ]}
          />
          <View style={styles.handle} />
          {children}
        </Pressable>
      </Pressable>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.3)',
    justifyContent: 'flex-end',
  },
  sheet: {
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
    paddingHorizontal: 16,
    paddingTop: 8,
    paddingBottom: 32,
    overflow: 'hidden',
  },
  sheetBlur: {
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
  },
  handle: {
    alignSelf: 'center',
    width: 36,
    height: 4,
    borderRadius: 2,
    backgroundColor: DynamicColorIOS({ light: '#C6C6C8', dark: '#5A5A5E' }),
    marginBottom: 12,
  },
});
