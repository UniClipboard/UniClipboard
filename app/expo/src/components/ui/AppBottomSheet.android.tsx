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
import { Gesture, GestureDetector, GestureHandlerRootView } from 'react-native-gesture-handler';
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';
import { duration } from '@/theme/motion';

export interface AppBottomSheetProps {
  visible: boolean;
  onDismiss: () => void;
  children: React.ReactNode;
  containerColor?: ColorValue;
}

/**
 * M3 风格底部弹层(纯 RN + Reanimated)。
 *
 * 不用 RN Modal 自带的 slide 动画(它会把 scrim 和面板一起从底部推上来,
 * 观感廉价),也不走 Compose ModalBottomSheet(RNHostView 跨边界的测量
 * 在设备上不可控)。这里 scrim 淡入、面板滑升两层分离,曲线对齐 M3:
 * 入场 emphasized-decelerate,退场 accelerate。
 * visible → false 时先播完退场动画再卸载。
 *
 * 顶部拖拽把手区域可下拉关闭(超过面板 1/4 高度或快速下甩即关闭,否则回弹),
 * 与 Compose ModalBottomSheet 的手势一致;把手对读屏暴露为「关闭」按钮。
 */

// 下拉关闭阈值:面板高度占比 / 甩动速度(px/s)
const DISMISS_DRAG_RATIO = 0.25;
const DISMISS_VELOCITY = 800;

// Reanimated worklet 版缓动,参数与 @/theme/motion 保持一致
const ENTER_EASING = Easing.bezier(0.2, 0, 0, 1);
const EXIT_EASING = Easing.bezier(0.4, 0, 1, 1);

export function AppBottomSheet({
  visible,
  onDismiss,
  children,
  containerColor,
}: AppBottomSheetProps) {
  const { t } = useTranslation('history');
  const { theme } = useTheme();
  const { height: windowHeight } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const [mounted, setMounted] = useState(visible);

  // 0 = 完全收起,1 = 完全展开;面板高度首帧布局前按整屏兜底(保证初始在屏外)
  const progress = useSharedValue(0);
  const panelHeight = useSharedValue(windowHeight);
  // 手势下拉的位移(px,>=0),叠加在进出场位移之上
  const dragY = useSharedValue(0);

  const unmount = useCallback(() => setMounted(false), []);

  useEffect(() => {
    if (visible) {
      if (!mounted) {
        // 首次打开:挂载后由 onLayout 拿到真实面板高度再启动入场
        setMounted(true);
      } else {
        // 退场中途重新打开:布局已知,直接续播回展开态
        progress.value = withTiming(1, {
          duration: duration.slow,
          easing: ENTER_EASING,
        });
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
        progress.value = withTiming(1, {
          duration: duration.slow,
          easing: ENTER_EASING,
        });
      }
    },
    [visible, panelHeight, progress]
  );

  const dragGesture = Gesture.Pan()
    .activeOffsetY([-8, 8])
    .onUpdate((e) => {
      dragY.value = Math.max(0, e.translationY);
    })
    .onEnd((e) => {
      const h = panelHeight.value || 1;
      if (dragY.value > h * DISMISS_DRAG_RATIO || e.velocityY > DISMISS_VELOCITY) {
        // 把拖拽位移折算回 progress,让退场动画从手指松开处续播,不跳帧
        progress.value = Math.max(0, 1 - dragY.value / h);
        dragY.value = 0;
        scheduleOnRN(onDismiss);
      } else {
        dragY.value = withSpring(0, { damping: 30, stiffness: 400 });
      }
    });

  const scrimStyle = useAnimatedStyle(() => ({
    opacity: progress.value * (1 - Math.min(1, dragY.value / (panelHeight.value || 1))),
  }));
  const panelStyle = useAnimatedStyle(() => ({
    transform: [{ translateY: (1 - progress.value) * panelHeight.value + dragY.value }],
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
      {/* RN Modal 是独立原生窗口,窗口内需要自己的手势根(拖拽把手、列表左滑) */}
      <GestureHandlerRootView style={StyleSheet.absoluteFill}>
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
              paddingBottom: Math.max(insets.bottom, 12),
              maxHeight: windowHeight * 0.9,
            },
            panelStyle,
          ]}
        >
          <GestureDetector gesture={dragGesture}>
            <View
              testID="app-bottom-sheet-handle"
              style={s.handleRow}
              accessible
              accessibilityRole="button"
              accessibilityLabel={t('action.close', { ns: 'common' })}
              onAccessibilityTap={onDismiss}
            >
              <View style={[s.handle, { backgroundColor: theme.colors.border }]} />
            </View>
          </GestureDetector>
          <View style={s.content}>{children}</View>
        </Animated.View>
      </GestureHandlerRootView>
    </Modal>
  );
}

const s = StyleSheet.create({
  panel: {
    position: 'absolute',
    bottom: 0,
    left: 0,
    right: 0,
    borderTopLeftRadius: 28,
    borderTopRightRadius: 28,
    overflow: 'hidden',
    elevation: 8,
  },
  // M3 drag handle:32×4,上下留出可拖拽区域
  handleRow: { alignItems: 'center', justifyContent: 'center', height: 36 },
  handle: { width: 32, height: 4, borderRadius: 2 },
  // wrap-content 面板里的内容区:可收缩,避免超出 maxHeight
  content: { flexGrow: 0, flexShrink: 1, flexBasis: 'auto' },
});
