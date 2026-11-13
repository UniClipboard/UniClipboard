import type { ActionMenuItem } from '@/utils/actionMenuItems';
import type { DisplayKind } from '@/utils/displayKind';

const QUICK_ACTION_KEYS: Record<DisplayKind, string[]> = {
  text: ['selectText', 'share'],
  url: ['openBrowser', 'share'],
  image: ['saveImage', 'share'],
  file: ['saveFile', 'share'],
  group: ['saveFile', 'share'],
};

const FALLBACK_QUICK_ACTION_KEYS = ['share', 'select'];

export interface DetailActionLayout {
  primary: ActionMenuItem | null;
  quick: ActionMenuItem[];
  overflow: ActionMenuItem[];
}

export function getDetailActionLayout(
  actions: ActionMenuItem[],
  displayKind: DisplayKind
): DetailActionLayout {
  const primary = actions.find((action) => action.key === 'copy') ?? null;
  const actionByKey = new Map(actions.map((action) => [action.key, action]));
  const quickKeys = [...QUICK_ACTION_KEYS[displayKind], ...FALLBACK_QUICK_ACTION_KEYS];
  const quick: ActionMenuItem[] = [];

  for (const key of quickKeys) {
    const action = actionByKey.get(key);
    if (action && !quick.some((candidate) => candidate.key === key)) {
      quick.push(action);
    }
    if (quick.length === 2) break;
  }

  const quickKeySet = new Set(quick.map((action) => action.key));
  const overflow = actions.filter(
    (action) => action.key !== primary?.key && !quickKeySet.has(action.key)
  );

  return { primary, quick, overflow };
}

/** 全屏详情页(Android)浮动工具栏的快捷动作,按内容类型取前几个存在的。 */
const PAGE_QUICK_ACTION_KEYS: Record<DisplayKind, string[]> = {
  text: ['selectText', 'sendTo', 'share'],
  url: ['selectText', 'sendTo', 'share'],
  image: ['saveImage', 'sendTo', 'share'],
  file: ['saveFile', 'sendTo', 'share'],
  group: ['saveFile', 'sendTo', 'share'],
};

/** 已由页面内容区直接承接的动作(如链接卡片上的「在浏览器打开」),不再进溢出菜单。 */
const PAGE_INLINE_ACTION_KEYS: Record<DisplayKind, string[]> = {
  text: [],
  url: ['openBrowser'],
  image: [],
  file: [],
  group: [],
};

export interface DetailPageActionLayout extends DetailActionLayout {
  /** 内容区内联的动作,按 key 取用 */
  inline: Record<string, ActionMenuItem>;
}

export function getDetailPageActionLayout(
  actions: ActionMenuItem[],
  displayKind: DisplayKind
): DetailPageActionLayout {
  const primary = actions.find((action) => action.key === 'copy') ?? null;
  const actionByKey = new Map(actions.map((action) => [action.key, action]));
  const quick = PAGE_QUICK_ACTION_KEYS[displayKind]
    .map((key) => actionByKey.get(key))
    .filter((action): action is ActionMenuItem => action != null);
  const inline: Record<string, ActionMenuItem> = {};
  for (const key of PAGE_INLINE_ACTION_KEYS[displayKind]) {
    const action = actionByKey.get(key);
    if (action) inline[key] = action;
  }
  const placed = new Set([
    primary?.key,
    ...quick.map((action) => action.key),
    ...Object.keys(inline),
  ]);
  const overflow = actions.filter((action) => !placed.has(action.key));
  return { primary, quick, overflow, inline };
}
