import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { Gesture } from 'react-native-gesture-handler';
import { useSharedValue } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import * as Haptics from 'expo-haptics';
import i18n from '@/i18n';
import { showUnavailable } from '@/shell/notice';
import { useMessageStore } from '@/stores/messageStore';

import {
  buildCopyText,
  getBandLayout,
  getSelectableIndices,
  getSelectionRuns,
  isSelectableToken,
  remapSelection,
  selectRange,
  tokenizeByChar,
  tokenizeWords,
  truncateForPicker,
  type CopyJoinMode,
  type SegToken,
} from '@/utils/wordSegmentation';



export type WordGranularity = 'word' | 'char';

export interface TokenFrame {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** 行索引：flex-wrap 的 tile 按行分桶，命中测试先找行带再在行内扫 x */
interface RowBucket {
  top: number;
  bottom: number;
  items: Array<{ index: number; left: number; right: number }>;
}

export interface WordPickerOptions {
  /**
   * 原地长按后拖动的语义：paint = 沿路径涂选（默认）；
   * range = 从按下的 token 到手指所在 token 之间整段选中 / 取消。
   */
  holdMode?: 'paint' | 'range';
  /**
   * 首尾拖柄的几何（相对 token 布局框）。传入后，选区只有一个连续段时
   * 按住拖柄可拖动调整该段的起点 / 终点；皮肤负责在同一位置画出拖柄。
   */
  handles?: {
    /** 拖柄圆心到词块边缘的竖直距离 */
    offsetY: number;
    hitRadius: number;
    /**
     * 起点拖柄在词块的哪条边:bottom(默认,两个拖柄都在下方);top 为系统文本选择的样式,
     * 起点圆点在首词上方、终点圆点在末词下方。
     */
    startEdge?: 'top' | 'bottom';
  };
}

interface HandlePoints {
  start: { x: number; y: number };
  end: { x: number; y: number };
}

const HIT_SLOP = 3;
/** 涂选扫掠的路径采样步长：快速甩动时按段插值命中，不跳 tile */
const SWEEP_STEP = 12;

function buildRowIndex(frames: Map<number, TokenFrame>, tokens: SegToken[]): RowBucket[] {
  // 标点也会注册布局（拖柄定位要用），但不参与命中
  const entries = [...frames.entries()]
    .filter(([index]) => tokens[index] && isSelectableToken(tokens[index]))
    .sort((a, b) => a[1].y - b[1].y || a[1].x - b[1].x);
  const rows: RowBucket[] = [];
  for (const [index, f] of entries) {
    const last = rows[rows.length - 1];
    const centerY = f.y + f.height / 2;
    if (last && centerY < last.bottom) {
      last.items.push({ index, left: f.x, right: f.x + f.width });
      last.top = Math.min(last.top, f.y);
      last.bottom = Math.max(last.bottom, f.y + f.height);
    } else {
      rows.push({
        top: f.y,
        bottom: f.y + f.height,
        items: [{ index, left: f.x, right: f.x + f.width }],
      });
    }
  }
  for (const row of rows) {
    row.items.sort((a, b) => a.left - b.left);
  }
  return rows;
}

/**
 * 分词选择浮层的交互状态机，iOS/Android 两个渲染层共用：
 * token 化（延迟到入场动画后）、选区、粒度切换重映射、
 * 涂选手势（横扫立即涂、原地长按 250ms 涂、竖滑让给滚动）、复制/分享。
 *
 * 命中测试基于 tile 相对流式容器的 onLayout 坐标——GestureDetector 挂在
 * 滚动内容里的容器上，手势坐标天然是内容系，无需滚动偏移与 measureInWindow。
 */
export function useWordPicker(
  text: string,
  close: (after?: () => void) => void,
  options: WordPickerOptions = {}
) {
  const { holdMode = 'paint', handles } = options;
  const { text: displayText, truncated } = useMemo(() => truncateForPicker(text), [text]);

  const [status, setStatus] = useState<'preparing' | 'ready'>('preparing');
  const [granularity, setGranularityState] = useState<WordGranularity>('word');
  const [tokens, setTokens] = useState<SegToken[]>([]);
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [joinMode, setJoinMode] = useState<CopyJoinMode>('original');

  // 涂选在手势回调里高频读写选区；ref 镜像避免闭包读到陈旧 state
  const selectedRef = useRef<ReadonlySet<number>>(selected);
  const tokenCacheRef = useRef<{ word?: SegToken[]; char?: SegToken[] }>({});
  const startedRef = useRef(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Soft).catch(() => {});
  }, []);

  /** 接入场动画完成回调；setTimeout(0) 让骨架屏先提交，再付词典加载的同步开销 */
  const beginTokenization = useCallback(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    setTimeout(() => {
      if (!mountedRef.current) return;
      const words = tokenizeWords(displayText);
      tokenCacheRef.current.word = words;
      setTokens(words);
      setStatus('ready');
    }, 0);
  }, [displayText]);

  const commitSelection = useCallback((next: ReadonlySet<number>) => {
    selectedRef.current = next;
    setSelected(next);
  }, []);

  const setGranularity = useCallback(
    (g: WordGranularity) => {
      if (g === granularity || status !== 'ready') return;
      const cache = tokenCacheRef.current;
      const next =
        g === 'word'
          ? (cache.word ??= tokenizeWords(displayText))
          : // 逐字粒度下标点也能单独选中
            (cache.char ??= tokenizeByChar(displayText, { selectablePunctuation: true }));
      commitSelection(remapSelection(tokens, selectedRef.current, next));
      setTokens(next);
      setGranularityState(g);
    },
    [granularity, status, tokens, displayText, commitSelection]
  );

  // ── 布局注册与命中测试 ────────────────────────────────
  const framesRef = useRef(new Map<number, TokenFrame>());
  const rowIndexRef = useRef<RowBucket[] | null>(null);

  // 拖柄位置依赖布局；同步函数在下方定义，经 ref 调用
  const syncHandlePointsRef = useRef<() => void>(() => {});
  const registerTokenLayout = useCallback((index: number, frame: TokenFrame) => {
    framesRef.current.set(index, frame);
    rowIndexRef.current = null;
    syncHandlePointsRef.current();
  }, []);

  // token 集变化时只清出界的旧条目：位置没变的 tile 不会重发 onLayout，
  // 整表清空会在这些 tile 上留下命中空洞
  useEffect(() => {
    const frames = framesRef.current;
    for (const index of frames.keys()) {
      if (index >= tokens.length || tokens[index].isWhitespace) frames.delete(index);
    }
    rowIndexRef.current = null;
  }, [tokens]);

  // 手势回调里读最新 token 集；提交后同步，避免渲染期写 ref
  const tokensRef = useRef(tokens);
  useLayoutEffect(() => {
    tokensRef.current = tokens;
  }, [tokens]);
  const getRows = useCallback(
    () => (rowIndexRef.current ??= buildRowIndex(framesRef.current, tokensRef.current)),
    []
  );

  const hitTest = useCallback(
    (x: number, y: number): number | null => {
      const rows = getRows();
      for (const row of rows) {
        if (y < row.top - HIT_SLOP || y > row.bottom + HIT_SLOP) continue;
        for (const item of row.items) {
          if (x >= item.left - HIT_SLOP && x <= item.right + HIT_SLOP) return item.index;
        }
        return null;
      }
      return null;
    },
    [getRows]
  );

  /** 拖动范围 / 拖柄用：手指在空隙或行外时取最近的行、行内最近的 token */
  const hitTestNearest = useCallback(
    (x: number, y: number): number | null => {
      const rows = getRows();
      let best: RowBucket | null = null;
      let bestDist = Infinity;
      for (const row of rows) {
        const dist = y < row.top ? row.top - y : y > row.bottom ? y - row.bottom : 0;
        if (dist < bestDist) {
          best = row;
          bestDist = dist;
        }
      }
      if (!best) return null;
      let nearest = best.items[0];
      let nearestDist = Infinity;
      for (const item of best.items) {
        const dist = x < item.left ? item.left - x : x > item.right ? x - item.right : 0;
        if (dist < nearestDist) {
          nearest = item;
          nearestDist = dist;
        }
      }
      return nearest.index;
    },
    [getRows]
  );

  // ── 涂选 ────────────────────────────────────────────
  const paintStateRef = useRef<{
    mode: 'select' | 'deselect';
    lastX: number;
    lastY: number;
  } | null>(null);

  const applyHits = useCallback(
    (hits: number[], mode: 'select' | 'deselect') => {
      if (hits.length === 0) return;
      let changed = false;
      const next = new Set(selectedRef.current);
      for (const idx of hits) {
        if (mode === 'select' ? !next.has(idx) : next.has(idx)) {
          if (mode === 'select') next.add(idx);
          else next.delete(idx);
          changed = true;
        }
      }
      if (!changed) return;
      commitSelection(next);
      Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {});
    },
    [commitSelection]
  );

  const beginPaint = useCallback(
    (x: number, y: number) => {
      const idx = hitTest(x, y);
      // 涂选方向 = 起点 token 状态取反；起点落空时默认涂选中
      const mode = idx !== null && selectedRef.current.has(idx) ? 'deselect' : 'select';
      paintStateRef.current = { mode, lastX: x, lastY: y };
      if (idx !== null) applyHits([idx], mode);
    },
    [hitTest, applyHits]
  );

  const paintTo = useCallback(
    (x: number, y: number) => {
      const st = paintStateRef.current;
      if (!st) return;
      const dx = x - st.lastX;
      const dy = y - st.lastY;
      const steps = Math.max(1, Math.ceil(Math.hypot(dx, dy) / SWEEP_STEP));
      const hits: number[] = [];
      for (let s = 1; s <= steps; s++) {
        const idx = hitTest(st.lastX + (dx * s) / steps, st.lastY + (dy * s) / steps);
        if (idx !== null && !hits.includes(idx)) hits.push(idx);
      }
      st.lastX = x;
      st.lastY = y;
      applyHits(hits, st.mode);
    },
    [hitTest, applyHits]
  );

  const endPaint = useCallback(() => {
    paintStateRef.current = null;
  }, []);

  // ── 范围选择（长按拖动 / 首尾拖柄）──────────────────────
  // base 是手势开始前的选区；range 模式下选区 = base ∪ / ∖ [anchor, focus]
  const rangeStateRef = useRef<{
    base: ReadonlySet<number>;
    anchor: number;
    focus: number;
    mode: 'select' | 'deselect';
    /** 拖柄拖动时手指在词块下方：按下点到被拖端词块中心的纵向偏移，命中时扣除 */
    dy: number;
  } | null>(null);
  const [rangeActive, setRangeActive] = useState(false);

  const applyRange = useCallback(
    (focus: number) => {
      const st = rangeStateRef.current;
      if (!st) return;
      st.focus = focus;
      const range = selectRange(tokensRef.current, st.anchor, focus);
      const next = new Set(st.base);
      for (const idx of range) {
        if (st.mode === 'select') next.add(idx);
        else next.delete(idx);
      }
      const prev = selectedRef.current;
      if (next.size === prev.size && [...next].every((i) => prev.has(i))) return;
      commitSelection(next);
      Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {});
    },
    [commitSelection]
  );

  const beginRange = useCallback(
    (x: number, y: number) => {
      const idx = hitTest(x, y);
      if (idx === null) return;
      const base = selectedRef.current;
      rangeStateRef.current = {
        base,
        anchor: idx,
        focus: idx,
        mode: base.has(idx) ? 'deselect' : 'select',
        dy: 0,
      };
      setRangeActive(true);
      applyRange(idx);
    },
    [hitTest, applyRange]
  );

  const rangeTo = useCallback(
    (x: number, y: number) => {
      const st = rangeStateRef.current;
      if (!st) return;
      const idx = hitTestNearest(x, y - st.dy);
      if (idx !== null) applyRange(idx);
    },
    [hitTestNearest, applyRange]
  );

  const endRange = useCallback(() => {
    rangeStateRef.current = null;
    setRangeActive(false);
    syncHandlePointsRef.current();
  }, []);

  /** 单一连续段时首尾拖柄圆心（token 流坐标）；其它情况为 null */
  const computeHandlePoints = useCallback((): HandlePoints | null => {
    if (!handles) return null;
    const runs = getSelectionRuns(tokensRef.current, selectedRef.current);
    if (runs.length !== 1) return null;
    const start = framesRef.current.get(runs[0].firstSelected);
    const end = framesRef.current.get(runs[0].lastSelected);
    if (!start || !end) return null;
    return {
      start: {
        x: start.x,
        y:
          handles.startEdge === 'top'
            ? start.y - handles.offsetY
            : start.y + start.height + handles.offsetY,
      },
      end: { x: end.x + end.width, y: end.y + end.height + handles.offsetY },
    };
  }, [handles]);

  // 拖柄位置镜像到 UI 线程，供手势 worklet 在按下时同步判定
  const handlePoints = useSharedValue<HandlePoints | null>(null);
  const syncHandlePoints = useCallback(() => {
    handlePoints.value = rangeStateRef.current ? null : computeHandlePoints();
  }, [handlePoints, computeHandlePoints]);
  useLayoutEffect(() => {
    syncHandlePointsRef.current = syncHandlePoints;
  }, [syncHandlePoints]);

  /** 按下点落在某个拖柄上时，以另一端为锚点开始范围拖动；否则返回 false */
  const beginHandleDrag = useCallback(
    (x: number, y: number): boolean => {
      const points = computeHandlePoints();
      if (!handles || !points) return false;
      const runs = getSelectionRuns(tokensRef.current, selectedRef.current);
      const [run] = runs;
      const distStart = Math.hypot(x - points.start.x, y - points.start.y);
      const distEnd = Math.hypot(x - points.end.x, y - points.end.y);
      if (Math.min(distStart, distEnd) > handles.hitRadius) return false;
      // 两个拖柄都在范围内（段很短）时取离手指更近的
      const dragEnd = distEnd <= distStart;
      const dragged = framesRef.current.get(dragEnd ? run.lastSelected : run.firstSelected)!;
      // 拖柄拖动期间拖柄保持可见并跟随两端，所以不置 rangeActive
      rangeStateRef.current = {
        base: new Set(),
        anchor: dragEnd ? run.firstSelected : run.lastSelected,
        focus: dragEnd ? run.lastSelected : run.firstSelected,
        mode: 'select',
        dy: y - (dragged.y + dragged.height / 2),
      };
      return true;
    },
    [handles, computeHandlePoints]
  );

  // 横向先动 → 立即涂选（此后二维自由跟踪）；纵向先动 → 交给滚动；
  // 原地按住 250ms → 涂选（RNGH：超时前位移过大则该手势失败，快速拖动仍是滚动）
  const paintGesture = useMemo(() => {
    const swipePan = Gesture.Pan()
      .activeOffsetX([-10, 10])
      .failOffsetY([-12, 12])
      .maxPointers(1)
      .runOnJS(true)
      .onStart((e) => beginPaint(e.x, e.y))
      .onUpdate((e) => paintTo(e.x, e.y))
      .onFinalize(endPaint);
    const holdPan = Gesture.Pan().activateAfterLongPress(250).maxPointers(1).runOnJS(true);
    if (holdMode === 'range') {
      holdPan
        .onStart((e) => beginRange(e.x, e.y))
        .onUpdate((e) => rangeTo(e.x, e.y))
        .onFinalize(endRange);
    } else {
      holdPan
        .onStart((e) => beginPaint(e.x, e.y))
        .onUpdate((e) => paintTo(e.x, e.y))
        .onFinalize(endPaint);
    }
    const paint = Gesture.Race(swipePan, holdPan);
    if (!handles) return paint;

    // 拖柄：按下即在 UI 线程判定（手动激活只能在 worklet 里调用），命中则立刻接管、
    // 压住滚动与点按；未命中立刻失败，不拖慢其它手势。后续回调转回 JS 线程
    const handlePan = Gesture.Pan()
      .manualActivation(true)
      .maxPointers(1)
      .onTouchesDown((e, manager) => {
        'worklet';
        const touch = e.allTouches[0];
        const points = handlePoints.value;
        const radius = handles.hitRadius;
        const near = (p: { x: number; y: number }) =>
          Math.hypot(touch.x - p.x, touch.y - p.y) <= radius;
        if (touch && points && (near(points.start) || near(points.end))) manager.activate();
        else manager.fail();
      })
      .onStart((e) => {
        'worklet';
        scheduleOnRN(beginHandleDrag, e.x, e.y);
      })
      .onUpdate((e) => {
        'worklet';
        scheduleOnRN(rangeTo, e.x, e.y);
      })
      .onEnd(() => {
        'worklet';
        // onEnd 只在激活过的手势结束时触发；按下即失败（没按在拖柄上）不会走到这里
        scheduleOnRN(endRange);
      });
    return Gesture.Exclusive(handlePan, paint);
  }, [
    holdMode,
    handles,
    beginPaint,
    paintTo,
    endPaint,
    beginRange,
    rangeTo,
    endRange,
    beginHandleDrag,
    handlePoints,
  ]);

  useEffect(() => {
    syncHandlePoints();
  }, [selected, tokens, syncHandlePoints]);

  // ── 选区派生与动作 ──────────────────────────────────
  const selectableIndices = useMemo(() => getSelectableIndices(tokens), [tokens]);
  const hasSelectableTokens = selectableIndices.length > 0;
  const allSelected = selected.size > 0 && selected.size === selectableIndices.length;

  const runs = useMemo(() => getSelectionRuns(tokens, selected), [tokens, selected]);
  const band = useMemo(() => getBandLayout(tokens, runs), [tokens, runs]);
  /** 只有一个连续段、且不在拖动范围时才显示首尾拖柄 */
  const handleRun = handles && !rangeActive && runs.length === 1 ? runs[0] : null;

  const previewText = useMemo(
    () => buildCopyText(displayText, tokens, selected, joinMode),
    [displayText, tokens, selected, joinMode]
  );
  const charCount = useMemo(() => countVisibleChars(previewText), [previewText]);

  // 预览编辑是对当前 previewText 的覆盖：选区或拼接方式一变，派生文本变了，编辑随之失效
  const [edit, setEdit] = useState<{ base: string; text: string } | null>(null);
  const isEdited = edit !== null && edit.base === previewText && edit.text !== previewText;
  const outputText = isEdited ? edit.text : previewText;
  const setOutputText = useCallback(
    (next: string) => setEdit({ base: previewText, text: next }),
    [previewText]
  );

  const toggleToken = useCallback(
    (index: number) => {
      const token = tokensRef.current[index];
      if (!token || !isSelectableToken(token)) return;
      const next = new Set(selectedRef.current);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      commitSelection(next);
    },
    [commitSelection]
  );

  const toggleSelectAll = useCallback(() => {
    commitSelection(allSelected ? new Set() : new Set(selectableIndices));
  }, [allSelected, selectableIndices, commitSelection]);

  const clearSelection = useCallback(() => commitSelection(new Set()), [commitSelection]);

  const copySelected = useCallback(async () => { if (outputText) showUnavailable(); }, [outputText]);
  const shareSelected = copySelected;
  const searchSelected = copySelected;

  return {
    status,
    truncated,
    displayText,
    granularity,
    setGranularity,
    tokens,
    selected,
    selectedCount: selected.size,
    hasSelectableTokens,
    allSelected,
    previewText,
    charCount,
    runs,
    band,
    handleRun,
    joinMode,
    setJoinMode,
    outputText,
    setOutputText,
    isEdited,
    beginTokenization,
    toggleToken,
    toggleSelectAll,
    clearSelection,
    copySelected,
    shareSelected,
    searchSelected,
    registerTokenLayout,
    paintGesture,
  };
}

/** 非空白字符数（按码点计，emoji 算一个） */
export function countVisibleChars(text: string): number {
  let count = 0;
  for (const ch of text) {
    if (!/\s/.test(ch)) count++;
  }
  return count;
}
