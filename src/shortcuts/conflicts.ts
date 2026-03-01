import { DEFAULT_SCOPE_LAYER, ShortcutDefinition, ShortcutScope } from './definitions'
import { ShortcutLayer, LAYER_ORDER } from './layers'
import { normalizeHotkey } from './normalize'

export type ShortcutKeyOverrides = Record<string, string>

export type ResolvedShortcut = ShortcutDefinition & {
  resolvedKey: string
  normalizedKey: string
  layer: ShortcutLayer
}

export type SameScopeConflict = {
  scope: ShortcutScope
  layer: ShortcutLayer
  normalizedKey: string
  shortcuts: ResolvedShortcut[]
}

export type ShadowingInfo = {
  normalizedKey: string
  higherLayer: ShortcutLayer
  lowerLayer: ShortcutLayer
  higher: ResolvedShortcut[]
  lower: ResolvedShortcut[]
}

/**
 * 将默认定义与用户覆盖（自定义键位）合并为可分析的 resolved 列表
 */
export const resolveShortcuts = (
  definitions: ShortcutDefinition[],
  overrides: ShortcutKeyOverrides = {},
  scopeLayer: Record<ShortcutScope, ShortcutLayer> = DEFAULT_SCOPE_LAYER
): ResolvedShortcut[] => {
  return definitions.map(def => {
    const rawKey = overrides[def.id] ?? def.key
    const resolvedKey = Array.isArray(rawKey) ? (rawKey[0] ?? '') : rawKey
    const normalizedKey = normalizeHotkey(resolvedKey)
    const layer = scopeLayer[def.scope] ?? 'page'
    return { ...def, resolvedKey, normalizedKey, layer }
  })
}

/**
 * 分析快捷键冲突/遮蔽信息，便于设置页即时提示与保存校验
 */
export const analyzeShortcutConflicts = (
  resolved: ResolvedShortcut[]
): {
  sameScopeConflicts: SameScopeConflict[]
  shadowing: ShadowingInfo[]
} => {
  const byScopeKey = new Map<string, ResolvedShortcut[]>()
  const byKeyLayer = new Map<string, Map<ShortcutLayer, ResolvedShortcut[]>>()

  for (const s of resolved) {
    if (!s.normalizedKey) continue

    const scopeKey = `${s.scope}::${s.normalizedKey}`
    byScopeKey.set(scopeKey, [...(byScopeKey.get(scopeKey) ?? []), s])

    const layerMap = byKeyLayer.get(s.normalizedKey) ?? new Map<ShortcutLayer, ResolvedShortcut[]>()
    layerMap.set(s.layer, [...(layerMap.get(s.layer) ?? []), s])
    byKeyLayer.set(s.normalizedKey, layerMap)
  }

  const sameScopeConflicts: SameScopeConflict[] = []
  for (const shortcuts of byScopeKey.values()) {
    if (shortcuts.length <= 1) continue
    sameScopeConflicts.push({
      scope: shortcuts[0]!.scope,
      layer: shortcuts[0]!.layer,
      normalizedKey: shortcuts[0]!.normalizedKey,
      shortcuts,
    })
  }

  const shadowing: ShadowingInfo[] = []
  for (const [normalizedKey, layerMap] of byKeyLayer.entries()) {
    const layers = Array.from(layerMap.keys()).sort((a, b) => LAYER_ORDER[b] - LAYER_ORDER[a])
    for (let i = 0; i < layers.length; i++) {
      for (let j = i + 1; j < layers.length; j++) {
        const higherLayer = layers[i]!
        const lowerLayer = layers[j]!
        const higher = layerMap.get(higherLayer) ?? []
        const lower = layerMap.get(lowerLayer) ?? []
        if (higher.length === 0 || lower.length === 0) continue
        shadowing.push({
          normalizedKey,
          higherLayer,
          lowerLayer,
          higher,
          lower,
        })
      }
    }
  }

  return { sameScopeConflicts, shadowing }
}

/**
 * 给“单个候选键位”做即时校验（用于 key picker）
 */
export const getCandidateKeyIssues = (
  resolved: ResolvedShortcut[],
  candidate: { id: string; scope: ShortcutScope; key: string }
): { level: 'error' | 'warning' | 'info'; message: string; relatedIds: string[] }[] => {
  const normalized = normalizeHotkey(candidate.key)
  if (!normalized) return []

  const candidateLayer =
    resolved.find(s => s.id === candidate.id)?.layer ?? DEFAULT_SCOPE_LAYER[candidate.scope]

  const sameScope = resolved.filter(
    s => s.id !== candidate.id && s.scope === candidate.scope && s.normalizedKey === normalized
  )
  if (sameScope.length > 0) {
    return [
      {
        level: 'error',
        message: `同一作用域内快捷键冲突：${normalized}`,
        relatedIds: sameScope.map(s => s.id),
      },
    ]
  }

  const sameLayerOtherScopes = resolved.filter(
    s => s.id !== candidate.id && s.layer === candidateLayer && s.normalizedKey === normalized
  )
  const issues: { level: 'error' | 'warning' | 'info'; message: string; relatedIds: string[] }[] =
    []

  if (sameLayerOtherScopes.length > 0) {
    issues.push({
      level: 'warning',
      message: `同一层级内其它作用域也使用了该快捷键：${normalized}`,
      relatedIds: sameLayerOtherScopes.map(s => s.id),
    })
  }

  const higherLayerShadow = resolved.filter(
    s =>
      s.id !== candidate.id &&
      s.normalizedKey === normalized &&
      LAYER_ORDER[s.layer] > LAYER_ORDER[candidateLayer]
  )
  if (higherLayerShadow.length > 0) {
    issues.push({
      level: 'info',
      message: `当更高层级激活时会被遮蔽：${normalized}`,
      relatedIds: higherLayerShadow.map(s => s.id),
    })
  }

  const lowerLayerShadowed = resolved.filter(
    s =>
      s.id !== candidate.id &&
      s.normalizedKey === normalized &&
      LAYER_ORDER[s.layer] < LAYER_ORDER[candidateLayer]
  )
  if (lowerLayerShadowed.length > 0) {
    issues.push({
      level: 'info',
      message: `会遮蔽更低层级中的快捷键：${normalized}`,
      relatedIds: lowerLayerShadowed.map(s => s.id),
    })
  }

  return issues
}
