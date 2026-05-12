import { useMemo } from 'react'
import { detectPlatformInfo, type PlatformInfo } from '@/lib/platform'

export type { PlatformInfo } from '@/lib/platform'

/**
 * 平台检测 Hook
 *
 * 提供集中的平台检测功能，用于实现跨平台的条件渲染和行为控制。
 *
 * @example
 * ```tsx
 * const { isWindows, isMac, isTauri } = usePlatform()
 *
 * if (isWindows && isTauri) {
 *   // Windows 特定逻辑
 * }
 * ```
 */
export const usePlatform = (): PlatformInfo => {
  return useMemo(() => detectPlatformInfo(), [])
}
