import {
  Command,
  HardDrive,
  Info,
  Palette,
  RefreshCw,
  Settings,
  Shield,
  Wifi,
  type LucideIcon,
} from 'lucide-react'
import type { ComponentType } from 'react'
import AboutSection from './AboutSection'
import AppearanceSection from './AppearanceSection'
import GeneralSection from './GeneralSection'
import NetworkSection from './NetworkSection'
import SecuritySection from './SecuritySection'
import StorageSection from './StorageSection'
import SyncSection from './SyncSection'
import ShortcutsSection from '@/components/setting/ShortcutsSection'

export interface SettingsCategory {
  id: string
  icon: LucideIcon
  Component: ComponentType
}

export const SETTINGS_CATEGORIES: SettingsCategory[] = [
  {
    id: 'general',
    icon: Settings,
    Component: GeneralSection,
  },
  {
    id: 'appearance',
    icon: Palette,
    Component: AppearanceSection,
  },
  {
    id: 'shortcuts',
    icon: Command,
    Component: ShortcutsSection,
  },
  {
    id: 'sync',
    icon: RefreshCw,
    Component: SyncSection,
  },
  {
    id: 'security',
    icon: Shield,
    Component: SecuritySection,
  },
  {
    id: 'network',
    icon: Wifi,
    Component: NetworkSection,
  },
  {
    id: 'storage',
    icon: HardDrive,
    Component: StorageSection,
  },
  {
    id: 'about',
    icon: Info,
    Component: AboutSection,
  },
]

export const DEFAULT_CATEGORY = SETTINGS_CATEGORIES[0].id
