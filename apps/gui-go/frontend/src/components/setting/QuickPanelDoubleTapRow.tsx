import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  getQuickPanelDoubleTapAvailability,
  type ModifierDoubleTapAvailability,
} from '@/api/tauri-command'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { QuickPanelDoubleTapModifier } from '@/types/setting'
import { SettingSelect } from './SettingControls'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'
import type { AccessibilityStatus } from './useQuickPanelAccessibility'
const log = createLogger('quick-panel-double-tap')

export function QuickPanelDoubleTapRow({
  enabled,
  accessibilityStatus,
}: {
  enabled: boolean
  accessibilityStatus: AccessibilityStatus
}) {
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
  // System permission is shared with the row above; display-session support remains a separate capability.
  const availability =
    isMac && doubleTapAvailability !== 'unsupported_display_session'
      ? ((
          {
            granted: 'supported',
            denied: 'accessibility_permission_required',
            checking: null,
            error: null,
          } as const
        )[accessibilityStatus] ?? null)
      : doubleTapAvailability
  const doubleTapSelectable =
    availability !== null && availability !== 'unsupported_display_session'
  const descriptionKeys = {
    checking: 'settings.sections.quickPanel.doubleTap.checking',
    supported: 'settings.sections.quickPanel.doubleTap.description',
    accessibility_permission_required: 'settings.sections.quickPanel.doubleTap.permissionRequired',
    unsupported_display_session: 'settings.sections.quickPanel.doubleTap.unsupported',
  }
  const doubleTapDescriptionKey = descriptionKeys[availability ?? 'checking']

  return (
    <SettingRow
      label={t('settings.sections.quickPanel.doubleTap.label')}
      description={t(doubleTapDescriptionKey)}
    >
      <div className="flex flex-wrap items-center justify-end gap-2">
        <SettingSelect
          label={t('settings.sections.quickPanel.doubleTap.label')}
          value={doubleTapModifier}
          onValueChange={value => setDoubleTapModifier(value as QuickPanelDoubleTapModifier)}
          disabled={!enabled || !doubleTapSelectable}
          items={[
            {
              value: 'disabled',
              label: t('settings.sections.quickPanel.doubleTap.disabled'),
            },
            { value: 'alt', label: altLabel },
            {
              value: 'control',
              label: t('settings.sections.quickPanel.doubleTap.control'),
            },
            { value: 'meta', label: metaLabel },
          ]}
        />
      </div>
    </SettingRow>
  )
}
