import { useCallback, useEffect, useRef } from 'react';

/** 两次点按间隔在此之内视为双击(与 Android ViewConfiguration.getDoubleTapTimeout 一致)。 */
export const DOUBLE_TAP_TIMEOUT_MS = 300;

/**
 * 在同一个 onPress 上区分单击与双击。
 *
 * 传入 onDoubleTap 时,单击要等双击窗口过去才触发;第二次点按落在窗口内则只触发双击。
 * 不传 onDoubleTap(如多选模式、iOS)时单击立即触发,没有额外延迟。
 */
export function useDoubleTap(
  onSingleTap: () => void,
  onDoubleTap?: () => void,
  timeoutMs = DOUBLE_TAP_TIMEOUT_MS
): () => void {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const singleRef = useRef(onSingleTap);
  singleRef.current = onSingleTap;

  const clear = useCallback(() => {
    if (timerRef.current != null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => clear, [clear]);

  const hasDoubleTap = onDoubleTap != null;
  useEffect(() => {
    // 切到不识别双击的状态(进入多选)时,丢掉还在等待的单击。
    if (!hasDoubleTap) clear();
  }, [hasDoubleTap, clear]);

  return useCallback(() => {
    if (!onDoubleTap) {
      singleRef.current();
      return;
    }
    if (timerRef.current != null) {
      clear();
      onDoubleTap();
      return;
    }
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      singleRef.current();
    }, timeoutMs);
  }, [onDoubleTap, clear, timeoutMs]);
}
