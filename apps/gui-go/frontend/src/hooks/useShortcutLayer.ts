import { useEffect } from 'react'
import { useShortcutContext } from '@/contexts/shortcut-context'
import { ShortcutScope } from '@/shortcuts/definitions'
import { ShortcutLayer } from '@/shortcuts/layers'

interface UseShortcutLayerOptions {
  layer: ShortcutLayer
  scope: ShortcutScope
  priority?: number
  enabled?: boolean
}

/**
 * 快捷键 layer/scope 管理 Hook
 *
 * 用于在组件生命周期内 push 一个 layer 上下文，并在卸载时自动 pop。
 * 支持 modal/page/global 三层，以及同层 priority 决定哪个 scope 激活。
 */
export const useShortcutLayer = ({
  layer,
  scope,
  priority = 0,
  enabled = true,
}: UseShortcutLayerOptions): void => {
  const { pushLayer, popLayer } = useShortcutContext()

  useEffect(() => {
    if (!enabled) return

    const token = pushLayer({ layer, scope, priority })
    return () => popLayer(token)
  }, [enabled, layer, scope, priority, pushLayer, popLayer])
}
