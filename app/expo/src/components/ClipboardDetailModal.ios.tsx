import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Modal, StyleSheet, View, useWindowDimensions } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  Easing,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { ClipboardDetailPane } from './ClipboardDetailPane';
import { ClipboardDetailPage } from './ios/ClipboardDetailPage';
import type { ClipboardItem } from '@/types/clipboard';
import type { ClipboardDetailModalProps } from './ClipboardDetailModal.types';

/** 推入 / 返回的时长与曲线,近似 UINavigationController 的转场 */
const PUSH_MS = 380;
const POP_MS = 260;
const PUSH_EASING = Easing.bezier(0.2, 0.9, 0.1, 1);
/** 左缘侧滑返回的起始热区宽度 */
const EDGE_WIDTH = 28;

/**
 * iOS 剪贴板详情:
 * - page(默认,手机单击卡片):推入式全屏页,从右侧滑入,左缘侧滑或返回按钮退出;
 * - sheet(iPad 窄屏的选中项):原生 pageSheet 里的详情面板。
 * 退场期间 item 可能已清空(关闭、删除),保留最后一条继续渲染直到滑出。
 */
export function ClipboardDetailModal(props: ClipboardDetailModalProps) {
  if (props.presentation === 'sheet') return <DetailSheet {...props} />;
  return <DetailPushPage {...props} />;
}

function DetailSheet({ visible, onDismiss, c, item, containerColor }: ClipboardDetailModalProps) {
  return (
    <Modal
      visible={visible}
      animationType="slide"
      presentationStyle="pageSheet"
      supportedOrientations={['portrait', 'landscape']}
      onRequestClose={onDismiss}
    >
      <View style={[styles.fill, { backgroundColor: containerColor ?? c.theme.colors.background }]}>
        <ClipboardDetailPane c={c} item={item ?? c.detailItem} onClose={onDismiss} />
      </View>
    </Modal>
  );
}

function DetailPushPage({ visible, onDismiss, c, item }: ClipboardDetailModalProps) {
  const { width } = useWindowDimensions();
  const current = item === undefined ? c.detailItem : item;
  const shown = visible ? current : null;
  const lastItemRef = useRef<ClipboardItem | null>(null);
  if (shown) lastItemRef.current = shown;
  const [mounted, setMounted] = useState(shown != null);
  const offset = useSharedValue(width);

  const unmount = useCallback(() => {
    setMounted(false);
    lastItemRef.current = null;
  }, []);

  // 打开:挂载后从右侧推入;外部关闭(返回、条目被删除)时滑出后卸载
  useEffect(() => {
    if (shown) {
      setMounted(true);
      offset.value = withTiming(0, { duration: PUSH_MS, easing: PUSH_EASING });
    } else if (mounted) {
      offset.value = withTiming(width, { duration: POP_MS, easing: Easing.out(Easing.quad) }, (done) => {
        if (done) scheduleOnRN(unmount);
      });
    }
    // mounted 不入依赖:开→挂载,关→播完退场再卸载
  }, [shown != null, offset, width, unmount]);

  const edgeSwipe = Gesture.Pan()
    .hitSlop({ left: 0, width: EDGE_WIDTH })
    .activeOffsetX(8)
    .failOffsetY([-20, 20])
    .onUpdate((event) => {
      offset.value = Math.max(0, event.translationX);
    })
    .onEnd((event) => {
      const shouldClose = event.translationX > width * 0.35 || event.velocityX > 800;
      if (shouldClose) {
        scheduleOnRN(onDismiss);
      } else {
        offset.value = withTiming(0, { duration: 200, easing: PUSH_EASING });
      }
    });

  const pageStyle = useAnimatedStyle(() => ({
    transform: [{ translateX: offset.value }],
    shadowOpacity: interpolate(offset.value, [0, width], [0.12, 0]),
  }));
  const dimStyle = useAnimatedStyle(() => ({
    opacity: interpolate(offset.value, [0, width], [0.12, 0]),
  }));

  const page = shown ?? lastItemRef.current;
  if (!mounted || !page) return null;

  return (
    <View style={StyleSheet.absoluteFill} pointerEvents={shown ? 'auto' : 'none'}>
      <Animated.View pointerEvents="none" style={[StyleSheet.absoluteFill, styles.dim, dimStyle]} />
      <GestureDetector gesture={edgeSwipe}>
        <Animated.View style={[styles.fill, styles.pageShadow, pageStyle]}>
          <ClipboardDetailPage c={c} item={page} onClose={onDismiss} />
        </Animated.View>
      </GestureDetector>
    </View>
  );
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  dim: { backgroundColor: '#000' },
  pageShadow: {
    shadowColor: '#000',
    shadowOffset: { width: -4, height: 0 },
    shadowRadius: 12,
  },
});
