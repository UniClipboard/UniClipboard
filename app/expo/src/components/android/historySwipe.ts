/**
 * 历史列表行的滑动判定(Android)。只看松手时行的实际位移,不计甩动速度:
 * 轻扫、斜向滚动带出的短横移都不会复制 / 删除,必须明确地把行拖过阈值。
 */

/** 手指横移超过它才开始识别滑动(点按、竖向滚动不会带出动作) */
export const SWIPE_ACTIVATION = 24;
/** 竖向偏移超过它就让给列表滚动 */
export const SWIPE_VERTICAL_SLOP = 12;
/** 行只跟随手指位移的 1/SWIPE_FRICTION */
export const SWIPE_FRICTION = 2;
/** 行位移越过它松手才执行;手指约需横向拖动 SWIPE_ACTIVATION + 2×80 ≈ 180dp */
export const SWIPE_TRIGGER = 80;
/** 行位移上限,露出完整的动作区 */
export const SWIPE_MAX = 112;

export type HistorySwipeAction = 'copy' | 'delete';

/** 手指位移 → 行位移(带阻力与上限) */
export function swipeOffset(fingerTranslationX: number): number {
  'worklet';
  const offset = fingerTranslationX / SWIPE_FRICTION;
  return Math.max(-SWIPE_MAX, Math.min(SWIPE_MAX, offset));
}

/** 松手时的行位移 → 动作:右滑复制、左滑删除,未过阈值不执行 */
export function resolveSwipeAction(rowOffset: number): HistorySwipeAction | null {
  'worklet';
  if (rowOffset >= SWIPE_TRIGGER) return 'copy';
  if (rowOffset <= -SWIPE_TRIGGER) return 'delete';
  return null;
}
