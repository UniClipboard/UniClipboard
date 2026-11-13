import React, {
  forwardRef,
  useCallback,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react';
import { LayoutChangeEvent, RefreshControlProps, ScrollView, StyleSheet, View } from 'react-native';
import Animated, { useAnimatedScrollHandler, useSharedValue } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { GridCell } from './GridCell';
import { buildOccurrenceKeys } from '@/utils/occurrenceKeys';
import type { PullToDismissHandlers } from '@/utils/pullToDismiss';

const OVERSCAN_MIN_PX = 400;

export interface AnimatedCardGridHandle {
  scrollToOffset: (options: { offset: number; animated?: boolean }) => void;
}

interface AnimatedCardGridProps<T> {
  items: T[];
  onVisibleItemsChanged?: (items: T[]) => void;
  numColumns: number;
  cardSize: number;
  renderCardSize?: number;
  spacing: number;
  paddingHorizontal: number;
  paddingTop: number;
  paddingBottom: number;
  keyExtractor: (item: T) => string;
  renderItem: (item: T) => React.ReactNode;
  refreshControl?: React.ReactElement<RefreshControlProps>;
  /**
   * iOS 专用:用 contentInset 在内容顶部预留空间(如筛选行 overlay)。
   * 与 paddingTop 的区别:UIRefreshControl 尊重 inset,下拉刷新的 spinner 不会被 overlay 遮挡。
   * scrollToOffset 的 offset 仍以内容坐标为准,组件内部会换算(offset 0 = 静止顶部)。
   */
  contentInsetTop?: number;
  /** UI 线程滚动回调(必须是 worklet),用于滚动联动 UI(如筛选行收展) */
  onScrollWorklet?: (y: number) => void;
  /** UI 线程滚动结束回调(必须是 worklet)。velocityY 非零表示拖拽松手后仍有惯性,终点以 momentumEnd(velocityY=0)为准 */
  onScrollEndWorklet?: (y: number, velocityY: number) => void;
  /** 在顶部下拉关闭的过程汇报 */
  pullToDismiss?: PullToDismissHandlers;
  /** Called once when each loaded batch is scrolled close to its end. */
  onEndReached?: () => void;
  onEndReachedThreshold?: number;
  /** 内容顶部、随内容滚动的页眉;占用的高度须由调用方计入 paddingTop */
  header?: React.ReactNode;
}

// 自研虚拟化网格：每张卡片的位置纯粹由它在 items 里的下标算出（行/列 = index / numColumns），
// 不依赖 FlatList 的 numColumns 整行打包，也不依赖任何原生 measure。下标变化时卡片用弹簧动画
// 平滑过渡到新坐标；目的地滚出屏幕时会被 ScrollView 原生裁剪，而不是飞到"假坐标"。
function AnimatedCardGridInner<T>(
  {
    items,
    onVisibleItemsChanged,
    numColumns,
    cardSize,
    renderCardSize,
    spacing,
    paddingHorizontal,
    paddingTop,
    paddingBottom,
    keyExtractor,
    renderItem,
    refreshControl,
    contentInsetTop = 0,
    onScrollWorklet,
    onScrollEndWorklet,
    pullToDismiss,
    onEndReached,
    onEndReachedThreshold = 400,
    header,
  }: AnimatedCardGridProps<T>,
  ref: React.Ref<AnimatedCardGridHandle>
) {
  const scrollRef = useRef<ScrollView>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const observedViewport = useRef({ top: 0, height: 0 });

  const cellSize = cardSize + spacing;
  const totalRows = Math.ceil(items.length / numColumns);
  const contentHeight = paddingTop + totalRows * cellSize + paddingBottom;
  const endReachedBatchKey = useMemo(() => {
    if (items.length === 0) return 'empty';
    const middleIndex = Math.floor(items.length / 2);
    return [
      items.length,
      keyExtractor(items[0]),
      keyExtractor(items[middleIndex]),
      keyExtractor(items[items.length - 1]),
    ].join(':');
  }, [items, keyExtractor]);

  // 渲染 key 经过出现序消歧:业务 key 出现重复(脏数据)时也保证 React key
  // 全局唯一,避免 key 冲突导致组件实例被过继给另一份副本(卡片乱飞/空洞)
  const cellKeys = useMemo(() => buildOccurrenceKeys(items, keyExtractor), [items, keyExtractor]);

  useImperativeHandle(
    ref,
    () => ({
      scrollToOffset: ({ offset, animated }) => {
        // 有 inset 时静止顶部在 -inset,offset 以内容坐标为准换算
        scrollRef.current?.scrollTo({ y: offset - contentInsetTop, animated: animated ?? true });
      },
    }),
    [contentInsetTop]
  );

  const reportVisibleItems = useCallback((top: number, height: number) => {
    if (!onVisibleItemsChanged) return;
    // Report native viewport observations only; exclude cells mounted for overscan.
    const start = Math.max(0, Math.floor((top - paddingTop) / cellSize)) * numColumns;
    const end = height > 0
      ? Math.max(0, Math.ceil((top - paddingTop + height) / cellSize)) * numColumns
      : 0;
    onVisibleItemsChanged(items.slice(start, end));
  }, [items, paddingTop, cellSize, numColumns, onVisibleItemsChanged]);

  const handleScrollPosition = useCallback((top: number) => {
    observedViewport.current.top = top;
    setScrollTop(top);
    reportVisibleItems(top, observedViewport.current.height);
  }, [reportVisibleItems]);

  const reportViewport = useCallback(() => {
    const { top, height } = observedViewport.current;
    reportVisibleItems(top, height);
  }, [reportVisibleItems]);

  // 滚动追踪放在 UI 线程（worklet）而不是 JS 线程的 onScroll：快速甩动时 JS 线程可能忙于
  // 渲染/状态更新而丢帧或延迟处理原生滚动事件，导致虚拟化窗口跟不上真实滚动位置，
  // 表现为卡片挂载/卸载错乱。UI 线程里做节流判断，只有跨过半行阈值才回传 JS 更新状态。
  const lastReportedScrollTop = useSharedValue(0);
  const pulling = useSharedValue(false);
  const lastEndReachedBatchKey = useSharedValue('');
  const scrollHandler = useAnimatedScrollHandler(
    {
      onScroll: (event) => {
        const y = event.contentOffset.y;
        onScrollWorklet?.(y);
        // 下拉关闭:只在越过顶部期间回传,回到顶部时再报一次收尾;静止顶部在 -contentInsetTop
        if (pullToDismiss) {
          const pull = y + contentInsetTop;
          if (pull < 0 || pulling.value) {
            pulling.value = pull < 0;
            scheduleOnRN(pullToDismiss.onPull, pull);
          }
        }
        if (Math.abs(lastReportedScrollTop.value - y) > cellSize / 2) {
          lastReportedScrollTop.value = y;
          scheduleOnRN(handleScrollPosition, y);
        }
        if (
          onEndReached &&
          viewportHeight > 0 &&
          contentHeight > viewportHeight &&
          y + viewportHeight + onEndReachedThreshold >= contentHeight &&
          lastEndReachedBatchKey.value !== endReachedBatchKey
        ) {
          lastEndReachedBatchKey.value = endReachedBatchKey;
          scheduleOnRN(onEndReached);
        }
      },
      onEndDrag: (event) => {
        onScrollEndWorklet?.(event.contentOffset.y, event.velocity?.y ?? 0);
        if (pullToDismiss) scheduleOnRN(pullToDismiss.onRelease, event.contentOffset.y + contentInsetTop);
      },
      onMomentumEnd: (event) => {
        onScrollEndWorklet?.(event.contentOffset.y, 0);
      },
    },
    [
      handleScrollPosition,
      onScrollWorklet,
      onScrollEndWorklet,
      pullToDismiss,
      contentInsetTop,
      onEndReached,
      onEndReachedThreshold,
      viewportHeight,
      contentHeight,
      endReachedBatchKey,
      cellSize,
    ]
  );

  const handleLayout = useCallback((e: LayoutChangeEvent) => {
    const height = e.nativeEvent.layout.height;
    observedViewport.current.height = height;
    setViewportHeight(height);
    reportVisibleItems(observedViewport.current.top, height);
  }, [reportVisibleItems]);

  const overscan = Math.max(viewportHeight, OVERSCAN_MIN_PX);
  const windowStartRow = Math.max(0, Math.floor((scrollTop - paddingTop - overscan) / cellSize));
  const windowEndRow = Math.min(
    totalRows,
    Math.ceil((scrollTop - paddingTop + viewportHeight + overscan) / cellSize)
  );

  const windowStartIndex = windowStartRow * numColumns;
  const windowEndIndex = Math.min(items.length, windowEndRow * numColumns);

  // 只渲染可视窗口与缓冲区。GridCell 的静态坐标始终是最终槽位，移动动画只是
  // 从旧槽位到最终槽位的视觉补偿，因此无需在动画开始/结束时回写列表状态。
  const renderIndices = useMemo(() => {
    return Array.from(
      { length: Math.max(0, windowEndIndex - windowStartIndex) },
      (_, offset) => windowStartIndex + offset
    );
  }, [windowStartIndex, windowEndIndex]);

  return (
    <Animated.ScrollView
      ref={scrollRef}
      onScroll={scrollHandler}
      onLayout={handleLayout}
      onContentSizeChange={reportViewport}
      scrollEventThrottle={1}
      refreshControl={refreshControl}
      showsVerticalScrollIndicator={false}
      keyboardDismissMode="on-drag"
      keyboardShouldPersistTaps="handled"
      contentInset={contentInsetTop > 0 ? { top: contentInsetTop } : undefined}
      contentOffset={contentInsetTop > 0 ? { x: 0, y: -contentInsetTop } : undefined}
      automaticallyAdjustContentInsets={false}
    >
      <View style={{ height: contentHeight }}>
        {header ? <View style={styles.header}>{header}</View> : null}
        {renderIndices.map((i) => {
          const item = items[i];
          const cellKey = cellKeys[i];
          return (
            <GridCell
              key={cellKey}
              index={i}
              item={item}
              renderItem={renderItem}
              onLayout={reportViewport}
              numColumns={numColumns}
              cardSize={cardSize}
              renderCardSize={renderCardSize}
              spacing={spacing}
              paddingHorizontal={paddingHorizontal}
              paddingTop={paddingTop}
            />
          );
        })}
      </View>
    </Animated.ScrollView>
  );
}

export const AnimatedCardGrid = forwardRef(AnimatedCardGridInner) as <T>(
  props: AnimatedCardGridProps<T> & { ref?: React.Ref<AnimatedCardGridHandle> }
) => React.ReactElement;

const styles = StyleSheet.create({
  header: { position: 'absolute', top: 0, left: 0, right: 0 },
});
