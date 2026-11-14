import { useCallback, useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';

/** 低于此宽度时,快捷动作只显示图标、隐藏文字标签。 */
export const DETAIL_ACTION_BAR_COMPACT_WIDTH = 440;

/**
 * 详情操作栏两端(iOS/Android)共用的平台无关逻辑:按可用宽度判定是否进入紧凑模式。
 * 样式与图标映射仍由各平台文件自持——这里只收拢真正重复的行为。
 */
export function useDetailActionBarBehavior() {
  const [compact, setCompact] = useState(true);

  const handleLayout = useCallback((event: LayoutChangeEvent) => {
    setCompact(event.nativeEvent.layout.width < DETAIL_ACTION_BAR_COMPACT_WIDTH);
  }, []);

  return { compact, handleLayout };
}
