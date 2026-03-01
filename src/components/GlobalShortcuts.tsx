import { useLocation, useNavigate } from 'react-router-dom'
import { useShortcut } from '@/hooks/useShortcut'
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
      if (!settingsDef) {
        return
      }

      if (location.pathname !== '/settings') {
        navigate('/settings')
      }
    },
  })

  return null
}
