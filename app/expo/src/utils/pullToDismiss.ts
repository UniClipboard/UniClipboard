/** 滚动视图在顶部被下拉超过这段距离后松手,视为「下拉关闭」 */
export const PULL_TO_DISMISS_DISTANCE = 72;

/** 拖拽松手时的 contentOffset.y 是否达到下拉关闭的距离(可在 worklet 中调用) */
export function isPullToDismiss(offsetY: number): boolean {
  'worklet';
  return offsetY <= -PULL_TO_DISMISS_DISTANCE;
}

/**
 * 滚动视图向下拉关闭的使用方汇报拖拽过程。都在 JS 线程调用,offsetY 以静止顶部为 0。
 * - onPull:越过顶部下拉期间的每次滚动,以及回到顶部(offsetY >= 0)时的一次;
 * - onRelease:拖拽松手时。
 */
export interface PullToDismissHandlers {
  onPull: (offsetY: number) => void;
  onRelease: (offsetY: number) => void;
}
