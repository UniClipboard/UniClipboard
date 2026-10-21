import { useEffect, useState, type ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'
import { quickPanelUsesCompositorShortcuts } from '@/api/tauri-command'
import { usePlatform } from '@/hooks/usePlatform'
import { QuickPanelShortcutState } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ShortcutRow } from './ShortcutRow'
import { useQuickPanelShortcutStatus } from './useQuickPanelShortcutStatus'

const log = createLogger('quick-panel-shortcut-row')

export function QuickPanelShortcutRow(props: ComponentProps<typeof ShortcutRow>) {
  const { isLinux, isDesktopHost } = usePlatform()
  const { t } = useTranslation()
  const [compositorShortcuts, setCompositorShortcuts] = useState(false)
  const status = useQuickPanelShortcutStatus()
  useEffect(() => {
    if (!isLinux || !isDesktopHost) return
    let cancelled = false
    void quickPanelUsesCompositorShortcuts()
      .then(value => {
        if (!cancelled) setCompositorShortcuts(value)
      })
      .catch(err => log.warn({ err }, 'Failed to detect quick panel shortcut backend'))
    return () => {
      cancelled = true
    }
  }, [isLinux, isDesktopHost])

  return compositorShortcuts ? (
    <p className="text-ui-body text-muted-foreground">
      {t('settings.sections.quickPanel.compositorShortcuts')}
      <code className="mt-2 block select-text">uniclipboard --quick-panel</code>
    </p>
  ) : (
    <>
      <ShortcutRow {...props} />
      <ShortcutApplyNotice status={status} />
    </>
  )
}

/**
 * "Saved, not active": the saved shortcut is not the one the OS holds (another program holds it,
 * or the system refused it). The previous binding still works and is named. An unknown state is
 * said as such, not as success (ADR-013 section 10).
 */
function ShortcutApplyNotice({
  status,
}: {
  status: ReturnType<typeof useQuickPanelShortcutStatus>
}) {
  const { t } = useTranslation()
  if (!status) return null
  const active = (status.active ?? []).join(' / ')
  if (status.state === QuickPanelShortcutState.ShortcutStateNotActive) {
    return (
      <p role="status" className="text-ui-caption-relaxed text-destructive">
        {active
          ? t('settings.sections.shortcuts.notActive', { active, error: status.error })
          : t('settings.sections.shortcuts.notActiveNone', { error: status.error })}
      </p>
    )
  }
  if (status.state === QuickPanelShortcutState.ShortcutStateUnknown) {
    return (
      <p role="status" className="text-ui-caption-relaxed text-destructive">
        {t('settings.sections.shortcuts.applyUnknown', { error: status.error })}
      </p>
    )
  }
  return null
}
