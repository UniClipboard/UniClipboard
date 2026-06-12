import { createNavigationContainerRef } from '@react-navigation/native';

/**
 * 全局 navigation ref
 *
 * 用法：在 NavigationContainer 的 ref prop 上挂载，从 App 顶层（NavigationContainer 之外）
 * 通过此 ref 触发跳转，例如深链路由。
 */
export const navigationRef = createNavigationContainerRef();

export function navigateIfReady(name: string, params?: Record<string, unknown>): boolean {
  if (!navigationRef.isReady()) return false;
  // @react-navigation 的类型在 createNavigationContainerRef 无路由参数化时偏严
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (navigationRef.navigate as any)(name, params);
  return true;
}
