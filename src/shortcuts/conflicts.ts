import { DEFAULT_SCOPE_LAYER, ShortcutDefinition, ShortcutScope } from './definitions'
import { ShortcutLayer, LAYER_ORDER } from './layers'
import { normalizeHotkey } from './normalize'

export type ShortcutKeyOverrides = Record<string, string | string[]>

export type ResolvedShortcut = Omit<ShortcutDefinition, 'key'> & {
  key: string
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
  return definitions.flatMap(def => {
    const rawKey = overrides[def.id] ?? def.key
    const resolvedKeys = Array.isArray(rawKey) ? rawKey : [rawKey]
    const layer = scopeLayer[def.scope] ?? 'page'

    const seenNormalizedKeys = new Set<string>()
    const resolvedShortcuts: ResolvedShortcut[] = []

    for (const resolvedKey of resolvedKeys) {
      const normalizedKey = normalizeHotkey(resolvedKey)
      if (!normalizedKey || seenNormalizedKeys.has(normalizedKey)) {
        continue
      }

      seenNormalizedKeys.add(normalizedKey)
      resolvedShortcuts.push({ ...def, key: resolvedKey, resolvedKey, normalizedKey, layer })
    }

    return resolvedShortcuts
  })
}

/**
 * 给“单个候选键位”做即时校验（用于 key picker）
 */
export type CandidateKeyIssue = {
  level: 'error' | 'warning' | 'info'
  messageKey: string
  messageParams: Record<string, string>
  relatedIds: string[]
}

export const getCandidateKeyIssues = (
  resolved: ResolvedShortcut[],
  candidate: { id: string; scope: ShortcutScope; key: string }
): CandidateKeyIssue[] => {
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
        messageKey: 'settings.sections.shortcuts.issues.sameScope',
        messageParams: { key: normalized },
        relatedIds: sameScope.map(s => s.id),
      },
    ]
  }

  const sameLayerOtherScopes = resolved.filter(
    s => s.id !== candidate.id && s.layer === candidateLayer && s.normalizedKey === normalized
  )
  const issues: CandidateKeyIssue[] = []

  if (sameLayerOtherScopes.length > 0) {
    issues.push({
      level: 'warning',
      messageKey: 'settings.sections.shortcuts.issues.sameLayer',
      messageParams: { key: normalized },
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
      messageKey: 'settings.sections.shortcuts.issues.shadowedByHigher',
      messageParams: { key: normalized },
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
      messageKey: 'settings.sections.shortcuts.issues.shadowsLower',
      messageParams: { key: normalized },
      relatedIds: lowerLayerShadowed.map(s => s.id),
    })
  }

  return issues
}
