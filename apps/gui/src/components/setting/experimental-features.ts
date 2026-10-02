/**
 * Central registry of experimental setting keys.
 *
 * 数据驱动: 在此集中声明哪些设置项处于实验阶段。
 * SettingRow 通过 `experimentalKey` prop 查询本表，命中则自动渲染 ExperimentalBadge。
 *
 * Key 命名约定: `<section>.<field>` (e.g. `network.lanOnly`).
 */
export const EXPERIMENTAL_FEATURE_KEYS = ['network.lanOnly', 'network.allowOverlayAddrs'] as const

export type ExperimentalFeatureKey = (typeof EXPERIMENTAL_FEATURE_KEYS)[number]

const EXPERIMENTAL_SET: ReadonlySet<string> = new Set(EXPERIMENTAL_FEATURE_KEYS)

export function isExperimentalFeature(key: string | undefined): key is ExperimentalFeatureKey {
  return key !== undefined && EXPERIMENTAL_SET.has(key)
}
