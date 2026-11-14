import { useCallback, useEffect, useRef, useState } from 'react';

/** 复制成功后「已复制」标记的停留时长 */
export const COPIED_FEEDBACK_MS = 1200;

/**
 * 历史条目的复制动作 + 短暂的「已复制」状态(网格卡片与列表行共用)。
 * `copyItem` 返回是否成功;只有成功才亮起标记,失败提示由调用方(控制器)给出。
 */
export function useCopyFeedback<T>(item: T, copyItem?: (item: T) => Promise<boolean> | void) {
  const [justCopied, setJustCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    []
  );

  const copy = useCallback(async () => {
    if (!copyItem) return;
    const copied = await copyItem(item);
    if (!copied) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    setJustCopied(true);
    timerRef.current = setTimeout(() => setJustCopied(false), COPIED_FEEDBACK_MS);
  }, [copyItem, item]);

  return { justCopied, copy };
}
