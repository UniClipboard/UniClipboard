/**
 * iOS 历史列表行的滑动判定(UITableView 滑动操作的手感):行 1:1 跟手,
 * - 右滑(leading):露出「复制」,拖过按钮宽度松手即复制并弹回;
 * - 左滑(trailing):露出「更多 / 删除」,松手停在按钮全露的位置;拖过行宽的一半松手直接删除。
 */

/** 手指横移超过它才开始识别滑动(点按、竖向滚动不会带出动作) */
export const SWIPE_ACTIVATION = 12;
/** 竖向偏移超过它就让给列表滚动 */
export const SWIPE_VERTICAL_SLOP = 10;
/** 左侧「复制」按钮宽度 */
export const LEADING_ACTION_WIDTH = 88;
/** 右侧单个按钮宽度(「更多」「删除」各一个) */
export const TRAILING_BUTTON_WIDTH = 74;
export const TRAILING_ACTIONS_WIDTH = TRAILING_BUTTON_WIDTH * 2;
/** 左滑超过行宽的这个比例松手即删除 */
export const FULL_SWIPE_RATIO = 0.55;
/** 松手速度超过它时按方向判定(pt/s) */
const FLING_VELOCITY = 700;

export type RowSwipeOutcome =
  | { kind: 'close' }
  | { kind: 'copy' }
  | { kind: 'openTrailing' }
  | { kind: 'delete' };

/** 松手时的行位移与速度 → 结果 */
export function resolveRowSwipe(offset: number, velocityX: number, rowWidth: number): RowSwipeOutcome {
  'worklet';
  if (offset > 0) {
    return offset >= LEADING_ACTION_WIDTH ? { kind: 'copy' } : { kind: 'close' };
  }
  if (rowWidth > 0 && -offset >= rowWidth * FULL_SWIPE_RATIO) return { kind: 'delete' };
  if (velocityX > FLING_VELOCITY) return { kind: 'close' };
  if (-offset >= TRAILING_BUTTON_WIDTH || velocityX < -FLING_VELOCITY) return { kind: 'openTrailing' };
  return { kind: 'close' };
}

/** 滑动中是否已越过会触发动作的阈值(用于一次性的触感提示) */
export function isSwipeArmed(offset: number, rowWidth: number): boolean {
  'worklet';
  if (offset >= LEADING_ACTION_WIDTH) return true;
  return rowWidth > 0 && -offset >= rowWidth * FULL_SWIPE_RATIO;
}
