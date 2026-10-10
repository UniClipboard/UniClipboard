import { useEffect, useState, type ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { quickPanelUsesCompositorShortcuts } from '@/api/tauri-command'
import { usePlatform } from '@/hooks/usePlatform'
import { createLogger } from '@/lib/logger'
import { ShortcutRow } from './ShortcutRow'

const log = createLogger('quick-panel-shortcut-row')

export function QuickPanelShortcutRow(props: ComponentProps<typeof ShortcutRow>) {
  const { isLinux, isTauri } = usePlatform()
  const { t } = useTranslation()
  const [compositorShortcuts, setCompositorShortcuts] = useState(false)
  useEffect(() => {
    if (!isLinux || !isTauri) return
    let cancelled = false
    void quickPanelUsesCompositorShortcuts()
      .then(value => {
        if (!cancelled) setCompositorShortcuts(value)
      })
      .catch(err => log.warn({ err }, 'Failed to detect quick panel shortcut backend'))
    return () => {
      cancelled = true
    }
  }, [isLinux, isTauri])

  return compositorShortcuts ? (
    <p className="text-ui-body text-muted-foreground">
      {t('settings.sections.quickPanel.compositorShortcuts')}
      <code className="mt-2 block select-text">uniclipboard --quick-panel</code>
    </p>
  ) : (
    <ShortcutRow {...props} />
  )
}
