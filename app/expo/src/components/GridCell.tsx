import React from 'react';
import { StyleSheet, View } from 'react-native';
import Animated, { LinearTransition } from 'react-native-reanimated';

const GRID_CELL_LAYOUT_TRANSITION = LinearTransition.springify()
  .damping(20)
  .stiffness(200)
  .mass(0.8);

interface GridCellProps<T> {
  index: number;
  item: T;
  renderItem: (item: T) => React.ReactNode;
  onLayout?: () => void;
  numColumns: number;
  cardSize: number;
  renderCardSize?: number;
  spacing: number;
  paddingHorizontal: number;
  paddingTop: number;
}

// 静态 left/top 始终直接指向最终槽位；Reanimated 根据前后布局自动完成位置与尺寸过渡。
// 用 React.memo 包裹并把 renderItem(item) 挪到内部调用：快速滚动时虚拟化窗口频繁变化，
// 若父组件每次都重新生成子元素，已挂载、下标未变的卡片也会被迫整棵重渲染，
// 在 JS 线程本就紧张时进一步加重卡顿/错乱。
function GridCellInner<T>({
  index,
  item,
  renderItem,
  onLayout,
  numColumns,
  cardSize,
  renderCardSize,
  spacing,
  paddingHorizontal,
  paddingTop,
}: GridCellProps<T>) {
  const cellSize = cardSize + spacing;
  const col = index % numColumns;
  const row = Math.floor(index / numColumns);
  const targetX = paddingHorizontal + col * cellSize;
  const targetY = paddingTop + row * cellSize;

  return (
    <Animated.View
      layout={GRID_CELL_LAYOUT_TRANSITION}
      onLayout={onLayout}
      style={{
        position: 'absolute',
        left: targetX,
        top: targetY,
        width: cellSize,
        height: cellSize,
      }}
    >
      {renderCardSize ? (
        <View
          style={[
            styles.renderFrame,
            {
              left: spacing / 2,
              top: spacing / 2,
              width: renderCardSize,
              height: renderCardSize,
              transform: [{ scale: cardSize / renderCardSize }],
            },
          ]}
        >
          {renderItem(item)}
        </View>
      ) : (
        renderItem(item)
      )}
    </Animated.View>
  );
}

export const GridCell = React.memo(GridCellInner) as typeof GridCellInner;

const styles = StyleSheet.create({
  renderFrame: {
    position: 'absolute',
    transformOrigin: 'top left',
  },
});
