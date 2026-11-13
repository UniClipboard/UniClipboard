import type { SharedValue } from 'react-native-reanimated';


export interface HomeViewProps {
  onOpenSettings: () => void;
  onOpenAbout: () => void;
  /** 首页进入 / 退出搜索或多选态(Android 据此隐藏底部导航栏)。 */
  onImmersiveModeChange?: (immersive: boolean) => void;
  /** Android:添加菜单展开态的即时信号,悬浮导航胶囊据此让位于菜单遮罩。 */
  addMenuOpenSignal?: SharedValue<boolean>;
  /** iOS:标签栏搜索圆钮每按一次递增,首页据此进入搜索。 */
  searchRequestId?: number;
}
