import { createContext, use } from 'react'
import { ShortcutScope } from '@/shortcuts/definitions'
import { ShortcutLayer } from '@/shortcuts/layers'

/**
 * 快捷键上下文接口
 */
export interface ShortcutContextType {
  /** 当前激活的作用域（由 layer + priority 决定） */
  activeScope: ShortcutScope
  /** 当前激活的 layer（由 layer + priority 决定） */
  activeLayer: ShortcutLayer
  /** 当前激活的 priority（由 layer + priority 决定） */
  activePriority: number
  /** 推入一个 layer 上下文，返回 token 用于释放 */
  pushLayer: (entry: { layer: ShortcutLayer; scope: ShortcutScope; priority?: number }) => string
  /** 释放一个 layer 上下文（可乱序释放） */
  popLayer: (token: string) => void
}

/**
 * 快捷键上下文
 */
export const ShortcutContext = createContext<ShortcutContextType | undefined>(undefined)

/**
 * 使用快捷键上下文的 Hook
 */
export const useShortcutContext = (): ShortcutContextType => {
  const context = use(ShortcutContext)
  if (context === undefined) {
    throw new Error('useShortcutContext must be used within ShortcutProvider')
  }
  return context
}
