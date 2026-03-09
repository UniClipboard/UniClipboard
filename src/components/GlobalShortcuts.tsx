import { useLocation, useNavigate } from 'react-router-dom'
import { useShortcut } from '@/hooks/useShortcut'
import {
  getSettingsIconPosition,
  startCircularCollapse,
  startCircularReveal,
} from '@/lib/theme-transition'
import { SHORTCUT_DEFINITIONS } from '@/shortcuts/definitions'

/**
 * 全局快捷键注册组件
 *
 * 无渲染组件，集中注册所有 global scope 快捷键。
 * 必须放在 ShortcutProvider 和 Router 内部。
 */
export const GlobalShortcuts = () => {
  const navigate = useNavigate()
  const location = useLocation()
  const settingsDef = SHORTCUT_DEFINITIONS.find(d => d.id === 'nav.settings')
  const settingsShortcutEnabled = Boolean(settingsDef)

  useShortcut({
    key: settingsDef?.key ?? '',
    scope: 'global',
    enabled: settingsShortcutEnabled,
    handler: () => {
      const { x, y } = getSettingsIconPosition()
      if (location.pathname.startsWith('/settings')) {
        const doNavigate = () => {
          const idx = (window.history.state as { idx?: number } | null)?.idx
          if (typeof idx === 'number' && idx > 0) {
            navigate(-1)
          } else {
            navigate('/')
          }
        }
        startCircularCollapse(x, y, doNavigate)
      } else {
        startCircularReveal(x, y, () => navigate('/settings'))
      }
    },
  })

  return null
}
