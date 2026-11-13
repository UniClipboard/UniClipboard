import { iosDimensions } from '@/theme/iosDesignTokens';

const { gridAdaptiveMin, gridAdaptiveMax } = iosDimensions;

const WORKSPACE_GUTTER = 12;
const WORKSPACE_RAIL_WIDTH = 72;
const DETAIL_MIN_WIDTH = 300;
const DETAIL_MAX_WIDTH = 380;
const DETAIL_WIDTH_RATIO = 0.3;
const GRID_PADDING = 16;
const GRID_SPACING = 12;
const GRID_MIN_COLUMNS = 2;

export interface GridMetrics {
  numColumns: number;
  cardSize: number;
}

export interface ExpandedWorkspaceLayout {
  detailPlacement: 'side' | 'overlay';
  detailWidth: number;
  gridWidth: number;
}

export interface ExpandedWorkspaceOptions {
  /** 左侧是否有类型筛选栏。Android 用应用级 navigation rail,筛选改为网格顶部 chip 行。 */
  filterRail?: boolean;
}

export function computeExpandedWorkspaceLayout(
  screenWidth: number,
  { filterRail = true }: ExpandedWorkspaceOptions = {}
): ExpandedWorkspaceLayout {
  const detailWidth = Math.max(
    DETAIL_MIN_WIDTH,
    Math.min(DETAIL_MAX_WIDTH, Math.round(screenWidth * DETAIL_WIDTH_RATIO))
  );
  const minimumGridWidth =
    GRID_PADDING * 2 + GRID_SPACING * (GRID_MIN_COLUMNS - 1) + gridAdaptiveMin * GRID_MIN_COLUMNS;
  // 筛选栏连同它与网格之间的缝隙
  const railSpan = filterRail ? WORKSPACE_RAIL_WIDTH + WORKSPACE_GUTTER : 0;
  const sideDetailWidth =
    WORKSPACE_GUTTER * 2 + railSpan + WORKSPACE_GUTTER + detailWidth + minimumGridWidth;
  const detailPlacement = screenWidth >= sideDetailWidth ? 'side' : 'overlay';
  const gridWidth =
    screenWidth -
    WORKSPACE_GUTTER * 2 -
    railSpan -
    (detailPlacement === 'side' ? WORKSPACE_GUTTER + detailWidth : 0);

  return { detailPlacement, detailWidth, gridWidth };
}

/**
 * 按容器可用宽度反推自适应网格的列数与卡片边长。
 *
 * 目标:每张卡片落在 [gridAdaptiveMin, gridAdaptiveMax](160–210pt)之间。先用最大目标宽
 * 求出「至少需要几列」,再把剩余宽度均分给这些列。列数下限为 `minColumns`(手机保持 2)。
 *
 * 手机全屏(~390pt)→ 2 列;平板左栏(~420pt)→ 2–3 列;平板整宽 → 更多列。AnimatedCardGrid
 * 已完全参数化,拿到 numColumns/cardSize 即可正确布局,无需改动虚拟化逻辑。
 */
export function computeGridMetrics(
  availableWidth: number,
  padding: number,
  spacing: number,
  minColumns = 2
): GridMetrics {
  const contentWidth = availableWidth - padding * 2;
  if (contentWidth <= 0) {
    return { numColumns: minColumns, cardSize: 0 };
  }

  // 用最大目标卡宽求下限列数:contentWidth = n*card + (n-1)*spacing,card<=max
  // → n >= (contentWidth + spacing) / (max + spacing);取 ceil = 「保证卡片不超过 max 的最少列数」。
  // 注意必须 ceil 而非 floor:floor 会少算一列,让卡片撑破 max(平板宽栏下卡片被拉到 300+pt)。
  const columnsForMax = Math.ceil((contentWidth + spacing) / (gridAdaptiveMax + spacing));
  let numColumns = Math.max(minColumns, columnsForMax);

  // 若均分后卡片小于最小目标宽,减一列(但不低于 minColumns)
  const cardAt = (cols: number) => (contentWidth - spacing * (cols - 1)) / cols;
  while (numColumns > minColumns && cardAt(numColumns) < gridAdaptiveMin) {
    numColumns -= 1;
  }

  return { numColumns, cardSize: cardAt(numColumns) };
}
