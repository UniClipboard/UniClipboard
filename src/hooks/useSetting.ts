import { use } from 'react'
import { SettingContext } from '@/contexts/setting-context'
export type { SettingContextType } from '@/types/setting'
export type { Theme } from '@/types/setting'

/**
 * 使用设置上下文的钩子
 * @throws {Error} 如果在 SettingProvider 外部使用
 */
export const useSetting = () => {
  const context = use(SettingContext)

  if (context === undefined) {
    throw new Error('useSetting必须在SettingProvider内部使用')
  }
  return context
}
