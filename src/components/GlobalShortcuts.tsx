import { useLocation, useNavigate } from 'react-router-dom'
import { useShortcut } from '@/hooks/useShortcut'
import { adjustUiScale } from '@/lib/ui-scale'
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
  const zoomInDef = SHORTCUT_DEFINITIONS.find(d => d.id === 'global.zoomIn')
  const zoomOutDef = SHORTCUT_DEFINITIONS.find(d => d.id === 'global.zoomOut')
  const settingsShortcutEnabled = Boolean(settingsDef)
  const zoomInShortcutEnabled = Boolean(zoomInDef)
  const zoomOutShortcutEnabled = Boolean(zoomOutDef)

  useShortcut({
    key: settingsDef?.key ?? '',
    scope: 'global',
    id: 'nav.settings',
    enabled: settingsShortcutEnabled,
    handler: () => {
      if (location.pathname.startsWith('/settings')) {
        const idx = (window.history.state as { idx?: number } | null)?.idx
        if (typeof idx === 'number' && idx > 0) {
          navigate(-1)
        } else {
          navigate('/')
        }
      } else {
        navigate('/settings')
      }
    },
  })

  useShortcut({
    key: zoomInDef?.key ?? '',
    scope: 'global',
    id: 'global.zoomIn',
    enabled: zoomInShortcutEnabled,
    enableOnFormTags: true,
    handler: () => {
      adjustUiScale('in')
    },
  })

  useShortcut({
    key: zoomOutDef?.key ?? '',
    scope: 'global',
    id: 'global.zoomOut',
    enabled: zoomOutShortcutEnabled,
    enableOnFormTags: true,
    handler: () => {
      adjustUiScale('out')
    },
  })

  return null
}
