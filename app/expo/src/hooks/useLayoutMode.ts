import { useWindowDimensions } from 'react-native';

/**
 * 首页(以及任何需要平板双栏的界面)的响应式档位。只做两级:
 * - `compact`  : 手机竖/横屏、iPad 侧滑分屏、小平板竖屏 —— 完全沿用现有单栏布局。
 * - `expanded` : 平板横屏 / iPad 全屏 —— 主从双栏(左栏历史网格 + 右栏详情)。
 *
 * 用 useWindowDimensions 而非一次性静态判断:iPad 侧滑分屏 / 旋转会实时改变可用宽度,
 * 「平板」随时可能缩回「紧凑」,布局必须跟着窗口宽度走。
 */
export type LayoutMode = 'compact' | 'expanded';

/**
 * 双栏启用阈值(dp/pt,逻辑像素)。取 Android 公认的大屏门槛 sw600dp:
 * 手机竖屏(360–430dp)在其下走单栏;平板(含竖屏,如 Xiaomi Pad 5 = 711dp)在其上走双栏。
 * 注意:本 App orientation 锁 portrait,平板也只能竖屏,因此阈值必须覆盖平板竖屏宽度,
 * 若沿用 840(Material expanded)则锁竖屏的平板永远进不了双栏。
 */
export const EXPANDED_MIN_WIDTH = 600;

export function getLayoutMode(width: number): LayoutMode {
  return width >= EXPANDED_MIN_WIDTH ? 'expanded' : 'compact';
}

export function useLayoutMode(): LayoutMode {
  const { width } = useWindowDimensions();
  return getLayoutMode(width);
}
