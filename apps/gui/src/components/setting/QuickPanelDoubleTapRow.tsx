import { openUrl } from '@tauri-apps/plugin-opener'
import { ExternalLink } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getQuickPanelDoubleTapAvailability,
  type ModifierDoubleTapAvailability,
} from '@/api/tauri-command'
import {
  Button,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { QuickPanelDoubleTapModifier } from '@/types/setting'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'
const log = createLogger('quick-panel-double-tap')
const MACOS_ACCESSIBILITY_SETTINGS_URL =
  'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'

export function QuickPanelDoubleTapRow({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation()
  const { isMac, isWindows } = usePlatform()
  const { setting, updateQuickPanelSetting } = useSetting()
  const [doubleTapAvailability, setDoubleTapAvailability] =
    useState<ModifierDoubleTapAvailability | null>(null)

  useEffect(() => {
    let active = true
    const refreshAvailability = () => {
      void getQuickPanelDoubleTapAvailability()
        .then(availability => {
          if (active) setDoubleTapAvailability(availability)
        })
        .catch(err => {
          log.error({ err }, 'Failed to detect modifier double-tap availability')
          if (active) setDoubleTapAvailability('unsupported_display_session')
        })
    }

    refreshAvailability()
    window.addEventListener('focus', refreshAvailability)
    return () => {
      active = false
      window.removeEventListener('focus', refreshAvailability)
    }
  }, [])

  const [doubleTapModifier, setDoubleTapModifier] =
    useOptimisticSetting<QuickPanelDoubleTapModifier>(
      setting?.quickPanel?.doubleTapModifier ?? 'disabled',
      next => updateQuickPanelSetting({ doubleTapModifier: next }),
      {
        failureLog: 'Failed to change quick panel modifier trigger',
        errorKey: error =>
          typeof error === 'object' &&
          error !== null &&
          'code' in error &&
          error.code === 'AccessibilityPermissionRequired'
            ? 'settings.sections.quickPanel.doubleTap.permissionRequired'
            : 'settings.sections.general.saveError',
      }
    )
  const altLabel = isMac
    ? t('settings.sections.quickPanel.doubleTap.option')
    : t('settings.sections.quickPanel.doubleTap.alt')
  const metaLabel = isMac
    ? t('settings.sections.quickPanel.doubleTap.command')
    : isWindows
      ? t('settings.sections.quickPanel.doubleTap.windows')
      : t('settings.sections.quickPanel.doubleTap.super')
  const doubleTapSupported = doubleTapAvailability === 'supported'
  const doubleTapDescriptionKey =
    doubleTapAvailability === null
      ? 'settings.sections.quickPanel.doubleTap.checking'
      : doubleTapAvailability === 'accessibility_permission_required'
        ? 'settings.sections.quickPanel.doubleTap.permissionRequired'
        : doubleTapSupported
          ? 'settings.sections.quickPanel.doubleTap.description'
          : 'settings.sections.quickPanel.doubleTap.unsupported'

  const handleOpenAccessibilitySettings = () => {
    void openUrl(MACOS_ACCESSIBILITY_SETTINGS_URL).catch(err => {
      log.error({ err }, 'Failed to open macOS Accessibility settings')
      toast.error(t('settings.sections.quickPanel.doubleTap.openSettingsError'))
    })
  }

  return (
    <SettingRow
      label={t('settings.sections.quickPanel.doubleTap.label')}
      description={t(doubleTapDescriptionKey)}
    >
      <div className="flex flex-wrap items-center justify-end gap-2">
        {doubleTapAvailability === 'accessibility_permission_required' && (
          <Button variant="outline" size="sm" onClick={handleOpenAccessibilitySettings}>
            <ExternalLink className="size-3.5" />
            {t('settings.sections.quickPanel.doubleTap.openSettings')}
          </Button>
        )}
        <Select
          value={doubleTapModifier}
          onValueChange={value => setDoubleTapModifier(value as QuickPanelDoubleTapModifier)}
          disabled={!enabled || !doubleTapSupported}
        >
          <SelectTrigger className="h-9 w-40 ">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="disabled">
              {t('settings.sections.quickPanel.doubleTap.disabled')}
            </SelectItem>
            <SelectItem value="alt">{altLabel}</SelectItem>
            <SelectItem value="control">
              {t('settings.sections.quickPanel.doubleTap.control')}
            </SelectItem>
            <SelectItem value="meta">{metaLabel}</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </SettingRow>
  )
}
