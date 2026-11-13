import { Segment, useDefault } from 'segmentit';

/**
 * 分词选择浮层的纯逻辑核心：切词、字符区间、选区映射、复制文本拼装。
 *
 * 关键不变式：任何切词函数返回的 tokens 连续覆盖 [0, text.length)，
 * 即 tokens.map(t => t.text).join('') === text。segmentit 的 doSegment
 * 会静默丢弃空白字符（空格 / 换行），直接累加 token 长度推算字符位置
 * 会在含空白的文本上整体漂移——所以这里把分词结果对齐回原文，
 * 丢掉的空隙物化成显式的 whitespace token。
 */

export interface SegToken {
  text: string;
  /** [start, end) 在（截断后）原文中的精确字符区间 */
  start: number;
  end: number;
  /** 纯空白 token：不可选中、不参与命中测试，仅用于还原原文与布局断行 */
  isWhitespace: boolean;
  /** 纯标点 token：不可选中，但夹在选中 token 之间时随原文保留、不打断连续段 */
  isPunctuation: boolean;
}

/** 不连续选区（多个连续段）复制时的拼接方式 */
export type CopyJoinMode = 'original' | 'space' | 'newline';

/**
 * 选区中的一个连续段（token 下标，闭区间）。first/last 是高亮带的范围，
 * 可能延伸到段首尾被吸收的标点；firstSelected/lastSelected 是段内首尾的选中 token。
 */
export interface SelectionRun {
  first: number;
  last: number;
  firstSelected: number;
  lastSelected: number;
}

export const WORD_PICKER_MAX_CHARS = 5000;

const WHITESPACE_RE = /^\s+$/;
const PUNCTUATION_RE = /^\p{P}+$/u;
/** 句末标点：段尾紧跟它、且其后是空白或文末时并入该段（「数据。」而不是「数据」） */
const SENTENCE_END_RE = /[。．.！!？?…；;]/;

/** 超长文本截断（避免几万 token 的同步切词与渲染）；不劈开 surrogate pair */
export function truncateForPicker(text: string): { text: string; truncated: boolean } {
  if (text.length <= WORD_PICKER_MAX_CHARS) {
    return { text, truncated: false };
  }
  let end = WORD_PICKER_MAX_CHARS;
  const code = text.charCodeAt(end - 1);
  if (code >= 0xd800 && code <= 0xdbff) {
    end -= 1;
  }
  return { text: text.slice(0, end), truncated: true };
}

// 模块级懒单例：useDefault 同步加载全量词典（Hermes 上百 ms 量级），
// 调用方负责把首次调用挪到入场动画之后
let segmentInstance: Segment | null = null;
function getSegment(): Segment {
  if (!segmentInstance) {
    segmentInstance = useDefault(new Segment());
  }
  return segmentInstance;
}

function makeToken(text: string, start: number, selectablePunctuation = false): SegToken {
  return {
    text,
    start,
    end: start + text.length,
    isWhitespace: WHITESPACE_RE.test(text),
    isPunctuation: !selectablePunctuation && PUNCTUATION_RE.test(text),
  };
}

/**
 * 逐字模式：拉丁字母/数字连排与空白连排保持整体，其余（含 CJK）逐码点切分。
 * selectablePunctuation 为 true 时标点也是普通可选字符（分词选择的逐字粒度）。
 */
export function tokenizeByChar(
  text: string,
  { selectablePunctuation = false }: { selectablePunctuation?: boolean } = {}
): SegToken[] {
  const tokens: SegToken[] = [];
  // u flag 让 `.` 匹配完整码点，emoji 不会被劈成两个 surrogate 半块
  for (const m of text.matchAll(/[a-zA-Z0-9]+|\s+|./gu)) {
    tokens.push(makeToken(m[0], m.index ?? 0, selectablePunctuation));
  }
  return tokens;
}

/** 词模式：segmentit 分词后对齐回原文，物化被丢弃的空隙 */
export function tokenizeWords(text: string): SegToken[] {
  if (!text) return [];
  let words: string[];
  try {
    words = getSegment()
      .doSegment(text)
      .map((t) => t.w);
  } catch {
    return tokenizeByChar(text);
  }

  const tokens: SegToken[] = [];
  const pushGap = (from: number, to: number) => {
    // 空隙通常是被丢掉的空白；防御性地走逐字切分，任何内容都能还原
    for (const t of tokenizeByChar(text.slice(from, to))) {
      tokens.push(makeToken(t.text, t.start + from));
    }
  };

  let cursor = 0;
  for (const w of words) {
    if (!w) continue;
    const idx = text.indexOf(w, cursor);
    if (idx === -1) {
      // 输出与原文对不上（不应发生）：余下整体退化为逐字，保住覆盖不变式
      pushGap(cursor, text.length);
      cursor = text.length;
      break;
    }
    if (idx > cursor) pushGap(cursor, idx);
    tokens.push(makeToken(w, idx));
    cursor = idx + w.length;
  }
  if (cursor < text.length) pushGap(cursor, text.length);
  return tokens;
}

export function isSelectableToken(token: SegToken): boolean {
  return !token.isWhitespace && !token.isPunctuation;
}

export function getSelectableIndices(tokens: SegToken[]): number[] {
  const indices: number[] = [];
  for (let i = 0; i < tokens.length; i++) {
    if (isSelectableToken(tokens[i])) indices.push(i);
  }
  return indices;
}

/** 两端（任意顺序）之间的全部可选 token，用于长按拖动与首尾拖柄 */
export function selectRange(tokens: SegToken[], a: number, b: number): Set<number> {
  const result = new Set<number>();
  const from = Math.max(0, Math.min(a, b));
  const to = Math.min(tokens.length - 1, Math.max(a, b));
  for (let i = from; i <= to; i++) {
    if (isSelectableToken(tokens[i])) result.add(i);
  }
  return result;
}

/**
 * 选区 → 连续段。空白与标点不打断连续段；未选中的可选 token 打断。
 * 段尾紧跟的句末标点在其后为空白或文末时并入；段抵达文本首/尾的最后一个
 * 可选 token 时，把文本边缘的标点（如引号）一并并入，使全选复制还原原文。
 */
export function getSelectionRuns(
  tokens: SegToken[],
  selected: ReadonlySet<number>
): SelectionRun[] {
  const runs: SelectionRun[] = [];
  if (selected.size === 0) return runs;

  let current: SelectionRun | null = null;
  let firstSelectable = -1;
  let lastSelectable = -1;
  for (let i = 0; i < tokens.length; i++) {
    if (!isSelectableToken(tokens[i])) continue;
    if (firstSelectable === -1) firstSelectable = i;
    lastSelectable = i;
    if (selected.has(i)) {
      if (current) {
        current.last = i;
        current.lastSelected = i;
      } else {
        current = { first: i, last: i, firstSelected: i, lastSelected: i };
      }
    } else if (current) {
      runs.push(current);
      current = null;
    }
  }
  if (current) runs.push(current);

  for (const run of runs) {
    if (run.firstSelected === firstSelectable) {
      let j = run.first - 1;
      while (j >= 0 && !tokens[j].isWhitespace) {
        run.first = j;
        j--;
      }
    }
    let j = run.lastSelected + 1;
    while (j < tokens.length && tokens[j].isPunctuation) j++;
    const trailing = tokens.slice(run.lastSelected + 1, j);
    const atEdge = run.lastSelected === lastSelectable;
    const endsSentence =
      (j === tokens.length || tokens[j].isWhitespace) &&
      trailing.some((t) => SENTENCE_END_RE.test(t.text));
    if (trailing.length > 0 && (atEdge || endsSentence)) run.last = j - 1;
  }
  return runs;
}

/** 高亮带里一个 token 的拼接信息；不在带内的 token 为 null */
export interface BandSlot {
  /** 与前一个 token 连成一条（同一段、中间没有换行） */
  joinPrev: boolean;
  joinNext: boolean;
  /** 段的首个 / 最后一个选中 token：首尾拖柄挂在这里 */
  runStart: boolean;
  runEnd: boolean;
}

/**
 * 连续段 → 逐 token 的高亮带拼接信息。段内的空白、标点也在带内，
 * 使相邻选中词读起来是一整段文字；含换行的空白断开高亮带。
 */
export function getBandLayout(tokens: SegToken[], runs: SelectionRun[]): Array<BandSlot | null> {
  const band: Array<BandSlot | null> = new Array(tokens.length).fill(null);
  const breaksLine = (t: SegToken) => t.isWhitespace && t.text.includes('\n');
  for (const run of runs) {
    for (let i = run.first; i <= run.last; i++) {
      if (breaksLine(tokens[i])) continue;
      band[i] = {
        joinPrev: i > run.first && !breaksLine(tokens[i - 1]),
        joinNext: i < run.last && !breaksLine(tokens[i + 1]),
        runStart: i === run.firstSelected,
        runEnd: i === run.lastSelected,
      };
    }
  }
  return band;
}

/**
 * 分词↔逐字切换时按字符区间重叠把旧选区映射到新 token 集：
 * 旧选区每个连续段（含段内夹着的标点）覆盖的任一字符落进新 token 区间即选中，
 * 新 token 不可选（空白、不可选标点）的除外。这样逐字模式里段内的标点会一并选中，高亮不断开。
 */
export function remapSelection(
  oldTokens: SegToken[],
  selected: ReadonlySet<number>,
  newTokens: SegToken[]
): Set<number> {
  const result = new Set<number>();
  if (selected.size === 0) return result;

  const ranges: Array<[number, number]> = getSelectionRuns(oldTokens, selected).map((run) => [
    oldTokens[run.first].start,
    oldTokens[run.last].end,
  ]);

  // 旧选区区间互不重叠且已排序，新 token 也按 start 递增：双指针一遍扫完
  let ri = 0;
  for (let i = 0; i < newTokens.length; i++) {
    const t = newTokens[i];
    if (!isSelectableToken(t)) continue;
    while (ri < ranges.length && ranges[ri][1] <= t.start) ri++;
    if (ri < ranges.length && ranges[ri][0] < t.end) result.add(i);
  }
  return result;
}

/**
 * 选区 → 复制文本。每个连续段从原文逐字切片输出（段内的空白、标点随原文保留）。
 * 段与段之间按 join 拼接：original 依原文间隙取换行 / 空格 / 无分隔，
 * space 恒为空格，newline 恒为换行。
 */
export function buildCopyText(
  text: string,
  tokens: SegToken[],
  selected: ReadonlySet<number>,
  join: CopyJoinMode = 'newline'
): string {
  const runs = getSelectionRuns(tokens, selected);
  let out = '';
  let prevEnd = -1;
  for (const run of runs) {
    const start = tokens[run.first].start;
    const end = tokens[run.last].end;
    if (prevEnd !== -1) out += separatorFor(join, text.slice(prevEnd, start));
    out += text.slice(start, end);
    prevEnd = end;
  }
  return out;
}

function separatorFor(join: CopyJoinMode, gap: string): string {
  if (join === 'space') return ' ';
  if (join === 'newline') return '\n';
  const newlines = gap.match(/\n/g)?.length ?? 0;
  if (newlines >= 2) return '\n\n';
  if (newlines === 1) return '\n';
  return /\s/.test(gap) ? ' ' : '';
}
