/**
 * 快捷键 layer
 *
 * 用于解决不同 UI 层级的快捷键优先级：
 * modal > page > global
 */
export type ShortcutLayer = 'global' | 'page' | 'modal'

/**
 * layer 优先级顺序（数值越大越优先）
 */
export const LAYER_ORDER: Record<ShortcutLayer, number> = {
  global: 0,
  page: 100,
  modal: 200,
}
