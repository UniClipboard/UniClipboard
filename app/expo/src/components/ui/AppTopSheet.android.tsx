import React, { useCallback, useEffect, useState } from 'react';
import {
  Modal,
  Pressable,
  StyleSheet,
  useWindowDimensions,
  View,
  type ColorValue,
  type LayoutChangeEvent,
} from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';
import { duration } from '@/theme/motion';

export interface AppTopSheetProps {
  visible: boolean;
  onDismiss: () => void;
  children: React.ReactNode;
  containerColor?: ColorValue;
}

/**
 * M3 风格顶部下拉弹层(纯 RN + Reanimated)。
 *
 * 是 {@link AppBottomSheet} 的镜像:scrim 淡入、面板从顶部边缘滑下,
 * 抓手落在底缘。动画曲线一致(入场 emphasized-decelerate,退场 accelerate),
 * visible → false 时先播完退场再卸载。
 */
const ENTER_EASING = Easing.bezier(0.2, 0, 0, 1);
const EXIT_EASING = Easing.bezier(0.4, 0, 1, 1);

export function AppTopSheet({ visible, onDismiss, children, containerColor }: AppTopSheetProps) {
  const { t } = useTranslation('history');
  const { theme } = useTheme();
  const { height: windowHeight } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const [mounted, setMounted] = useState(visible);

  // 0 = 完全收起(在屏外上方),1 = 完全展开
  const progress = useSharedValue(0);
  const panelHeight = useSharedValue(windowHeight);

  const unmount = useCallback(() => setMounted(false), []);

  useEffect(() => {
    if (visible) {
      if (!mounted) {
        setMounted(true);
      } else {
        progress.value = withTiming(1, { duration: duration.slow, easing: ENTER_EASING });
      }
      return;
    }
    if (!mounted) return;
    progress.value = withTiming(0, { duration: duration.fast, easing: EXIT_EASING }, (finished) => {
      if (finished) scheduleOnRN(unmount);
    });
    // mounted 有意不进依赖:首帧挂载交给 onLayout 启动入场,避免用兜底高度起跳
  }, [visible, progress, unmount]);

  const handlePanelLayout = useCallback(
    (e: LayoutChangeEvent) => {
      panelHeight.value = e.nativeEvent.layout.height;
      if (visible && progress.value === 0) {
        progress.value = withTiming(1, { duration: duration.slow, easing: ENTER_EASING });
      }
    },
    [visible, panelHeight, progress]
  );

  const scrimStyle = useAnimatedStyle(() => ({ opacity: progress.value }));
  // 从顶部滑下:收起态整体上移一个面板高度(负向),展开归零
  const panelStyle = useAnimatedStyle(() => ({
    transform: [{ translateY: -(1 - progress.value) * panelHeight.value }],
  }));

  if (!mounted) return null;
  return (
    <Modal
      visible
      transparent
      animationType="none"
      statusBarTranslucent
      navigationBarTranslucent
      onRequestClose={onDismiss}
    >
      <Animated.View
        style={[StyleSheet.absoluteFill, { backgroundColor: theme.colors.backdrop }, scrimStyle]}
        pointerEvents="none"
      />
      <Pressable
        style={StyleSheet.absoluteFill}
        onPress={onDismiss}
        accessibilityRole="button"
        accessibilityLabel={t('action.close', { ns: 'common' })}
      />
      <Animated.View
        onLayout={handlePanelLayout}
        style={[
          s.panel,
          {
            backgroundColor: containerColor ?? theme.colors.surfaceLow,
            paddingTop: Math.max(insets.top, 12),
            maxHeight: windowHeight * 0.9,
          },
          panelStyle,
        ]}
      >
        {/* RN Modal 是独立原生窗口,窗口内需要自己的手势根(如列表左滑) */}
        <GestureHandlerRootView style={s.gestureRoot}>{children}</GestureHandlerRootView>
        <View style={s.handleRow}>
          <View style={[s.handle, { backgroundColor: theme.colors.separator }]} />
        </View>
      </Animated.View>
    </Modal>
  );
}

const s = StyleSheet.create({
  panel: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    borderBottomLeftRadius: 28,
    borderBottomRightRadius: 28,
    borderCurve: 'continuous',
    overflow: 'hidden',
    elevation: 8,
  },
  handleRow: { alignItems: 'center', paddingTop: 10, paddingBottom: 14 },
  handle: { width: 32, height: 4, borderRadius: 2 },
  // 覆盖 GestureHandlerRootView 默认 flex:1(basis 0 会把 wrap-content 面板压成 0 高)
  gestureRoot: { flexGrow: 0, flexShrink: 1, flexBasis: 'auto' },
});
