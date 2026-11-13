import { useEffect, useState } from 'react';
import AsyncStorage from '@react-native-async-storage/async-storage';

const GESTURES_LEARNED_KEY = '@syncclipboard:word-picker:gestures-learned';

/**
 * 分词选择的首次手势提示：用户第一次选中任何内容后记为已学会，
 * 下次打开不再显示。本次会话内保持显示，避免提示消失导致词块整体上移、打断正在进行的涂选。
 */
export function useWordPickerHint(hasSelection: boolean): boolean {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    let alive = true;
    AsyncStorage.getItem(GESTURES_LEARNED_KEY)
      .then((value) => {
        if (alive && value !== '1') setVisible(true);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (visible && hasSelection) AsyncStorage.setItem(GESTURES_LEARNED_KEY, '1').catch(() => {});
  }, [visible, hasSelection]);

  return visible;
}
