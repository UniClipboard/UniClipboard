import { createContext } from 'use-context-selector'
import type { SettingContextType } from '@/types/setting'

export const SettingContext = createContext<SettingContextType | undefined>(undefined)
