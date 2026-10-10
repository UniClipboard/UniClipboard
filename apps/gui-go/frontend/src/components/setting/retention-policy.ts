import type { RetentionPolicy, RetentionRule } from '@/types/setting'
export const SECONDS_PER_DAY = 86400
export const DEFAULT_RETENTION_POLICY: RetentionPolicy = {
  enabled: true,
  rules: [{ byAge: { maxAge: 30 * SECONDS_PER_DAY } }],
  skipPinned: true,
  evaluation: 'anyMatch',
}

export const RETENTION_DAYS_OPTIONS = [
  { value: '7', days: 7 },
  { value: '14', days: 14 },
  { value: '30', days: 30 },
  { value: '60', days: 60 },
  { value: '90', days: 90 },
  { value: '180', days: 180 },
  { value: '365', days: 365 },
] as const

export const MAX_ITEMS_OPTIONS = [
  { value: '100', count: 100 as number | null },
  { value: '200', count: 200 as number | null },
  { value: '500', count: 500 as number | null },
  { value: '1000', count: 1000 as number | null },
  { value: '2000', count: 2000 as number | null },
  { value: '5000', count: 5000 as number | null },
  { value: 'unlimited', count: null as number | null },
] as const

// ── Helpers ──────────────────────────────────────────────────────────

export function getByAgeSecs(rules: RetentionRule[]): number | null {
  for (const rule of rules) {
    if ('byAge' in rule) return rule.byAge.maxAge
  }
  return null
}

export function getByCountItems(rules: RetentionRule[]): number | null {
  for (const rule of rules) {
    if ('byCount' in rule) return rule.byCount.maxItems
  }
  return null
}

export function setByAgeRule(rules: RetentionRule[], days: number): RetentionRule[] {
  const newRule: RetentionRule = { byAge: { maxAge: days * SECONDS_PER_DAY } }
  return [newRule, ...rules.filter(r => !('byAge' in r))]
}

export function setByCountRule(rules: RetentionRule[], maxItems: number): RetentionRule[] {
  const newRule: RetentionRule = { byCount: { maxItems: maxItems } }
  return [...rules.filter(r => !('byCount' in r)), newRule]
}

/** Drop the `byCount` rule entirely — absence means "no count cap". */
export function clearByCountRule(rules: RetentionRule[]): RetentionRule[] {
  return rules.filter(r => !('byCount' in r))
}

/**
 * Resolve the persisted `byCount` value for a `MAX_ITEMS_OPTIONS` select
 * value; `null` means "no count cap" (the "unlimited" option).
 *
 * Falls back to 500 only if `value` doesn't match any known option (shouldn't
 * happen — the Select only offers known values). Note this must NOT be
 * written as `MAX_ITEMS_OPTIONS.find(...)?.count ?? 500`: when the option is
 * found and its `count` is legitimately `null`, `??` treats `null` as
 * nullish and incorrectly falls through to `500`.
 */
export function resolveMaxItemsCount(value: string): number | null {
  const opt = MAX_ITEMS_OPTIONS.find(o => o.value === value)
  return opt ? opt.count : 500
}
