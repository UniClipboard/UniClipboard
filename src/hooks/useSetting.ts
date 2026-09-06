import { useContextSelector } from 'use-context-selector'
import { SettingContext } from '@/contexts/setting-context'
import type { SettingContextType } from '@/types/setting'
export type { SettingContextType } from '@/types/setting'
export type { Theme } from '@/types/setting'

/** Subscribe only to a primitive or stable reference selected from settings. */
export function useSettingSelector<T>(selector: (context: SettingContextType) => T): T {
  return useContextSelector(SettingContext, context => {
    if (context === undefined) throw new Error('useSetting必须在SettingProvider内部使用')
    return selector(context)
  })
}

export const useSetting = () => useSettingSelector(context => context)
