/**
 * 快捷键动作类型
 * 所有快捷键动作的联合类型
 */
import { ShortcutLayer } from './layers'

export type ShortcutAction =
  | 'clipboard.clearSelection'
  | 'clipboard.selectAll'
  | 'clipboard.delete'
  | 'clipboard.favorite'
  | 'clipboard.copy'
  | 'navigation.dashboard'
  | 'navigation.devices'
  | 'navigation.settings'
  | 'search.focus'
  | 'modal.close'
  | string

/**
 * 快捷键作用域
 * 用于隔离不同页面/组件的快捷键
 */
export type ShortcutScope = 'global' | 'clipboard' | 'settings' | 'devices' | 'modal'

/**
 * 默认 scope -> layer 映射
 *
 * - global: 全局层（始终存在）
 * - page: 页面层（如 clipboard/settings/devices）
 * - modal: 模态层（打开模态框时）
 */
export const DEFAULT_SCOPE_LAYER: Record<ShortcutScope, ShortcutLayer> = {
  global: 'global',
  clipboard: 'page',
  settings: 'page',
  devices: 'page',
  modal: 'modal',
}

/**
 * 快捷键定义接口
 */
export interface ShortcutDefinition {
  /** 唯一标识符 */
  id: string
  /** 快捷键组合，如 "esc", "cmd+a", "mod+comma"，支持字符串或数组形式 */
  key: string | string[]
  /** 动作类型 */
  action: ShortcutAction
  /** 作用域 */
  scope: ShortcutScope
  /** 描述文本 */
  description: string
  /** 是否阻止默认行为 */
  preventDefault?: boolean
}

/**
 * 集中定义所有快捷键
 */
export const SHORTCUT_DEFINITIONS: ShortcutDefinition[] = [
  // ===== 剪贴板操作 =====
  {
    id: 'clipboard.esc',
    key: 'esc',
    action: 'clipboard.clearSelection',
    scope: 'clipboard',
    description: '取消选择',
  },
  // 预留更多快捷键位置
  // {
  //   id: "clipboard.selectAll",
  //   key: "cmd+a",
  //   action: "clipboard.selectAll",
  //   scope: "clipboard",
  //   description: "全选",
  // },
  // {
  //   id: "clipboard.delete",
  //   key: "backspace",
  //   action: "clipboard.delete",
  //   scope: "clipboard",
  //   description: "删除选中项",
  // },
  // {
  //   id: "clipboard.favorite",
  //   key: "cmd+f",
  //   action: "clipboard.favorite",
  //   scope: "clipboard",
  //   description: "收藏/取消收藏",
  // },

  // ===== 导航 =====
  // {
  //   id: "nav.dashboard",
  //   key: "cmd+1",
  //   action: "navigation.dashboard",
  //   scope: "global",
  //   description: "前往剪贴板",
  // },
  // {
  //   id: "nav.devices",
  //   key: "cmd+2",
  //   action: "navigation.devices",
  //   scope: "global",
  //   description: "前往设备",
  // },
  {
    id: 'nav.settings',
    key: 'mod+comma',
    action: 'navigation.settings',
    scope: 'global',
    description: '前往设置',
  },

  // ===== 搜索 =====
  // {
  //   id: "search.focus",
  //   key: "cmd+/",
  //   action: "search.focus",
  //   scope: "global",
  //   description: "聚焦搜索框",
  // },

  // ===== 模态框 =====
  // {
  //   id: "modal.close",
  //   key: "esc",
  //   action: "modal.close",
  //   scope: "modal",
  //   description: "关闭模态框",
  // },
]
