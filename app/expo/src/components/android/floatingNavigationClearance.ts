import { createContext, useContext } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

/**
 * 当前目的地之上悬浮导航胶囊额外占去的底部高度(dp,不含系统导航栏 inset)。
 * 不在顶级目的地(如 push 进来的设置二级页)或平板 rail 布局时为 0。
 *
 * 单独用 context 而非改写 SafeAreaInsets:目的地内弹出的底部面板仍需真实的 bottom inset。
 */
export const FloatingNavigationClearanceContext = createContext(0);

export function useFloatingNavigationClearance(): number {
  return useContext(FloatingNavigationClearanceContext);
}

/** Snackbar 的距底距离:有悬浮胶囊时抬到胶囊之上,否则交给 Snackbar 默认值。 */
export function useFloatingNavigationSnackbarOffset(): number | undefined {
  const clearance = useFloatingNavigationClearance();
  const insets = useSafeAreaInsets();
  return clearance > 0 ? insets.bottom + clearance + 8 : undefined;
}
