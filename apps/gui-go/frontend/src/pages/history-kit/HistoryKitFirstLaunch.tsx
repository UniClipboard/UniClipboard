import { useTranslation } from 'react-i18next'
import { useSettingSelector } from '@/hooks/useSetting'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { SHORTCUT_DEFINITIONS } from '@/shortcuts/definitions'
import { StepList } from '@/ui-kit'

const QUICK_PANEL = 'global.toggleQuickPanel'
const defaultQuickPanelKey = SHORTCUT_DEFINITIONS.find(d => d.id === QUICK_PANEL)?.key

/**
 * The empty history (design MA1): three getting-started steps. The first one waits for the first
 * copy, the second invites a device in Manage Devices, the third names the user's quick panel
 * shortcut. The design's "Try" button for the quick panel is not offered: the host has no command
 * that opens the quick panel from the page.
 */
export function HistoryKitFirstLaunch({ onPairDevice }: { onPairDevice: () => void }) {
  const { t } = useTranslation()
  const binding = useSettingSelector(
    ({ setting }) => setting?.keyboardShortcuts?.[QUICK_PANEL] ?? defaultQuickPanelKey ?? null
  )
  const shortcut = Array.isArray(binding) ? binding[0] : binding
  return (
    <StepList
      label={t('history.kit.gettingStarted')}
      steps={[
        {
          id: 'copy',
          title: t('history.kit.stepCopyTitle'),
          description: t('history.kit.stepCopyText', { shortcut: formatShortcutHint('mod+c') }),
          status: { text: t('history.kit.stepCopyWaiting'), active: true },
        },
        {
          id: 'pair',
          title: t('history.kit.stepPairTitle'),
          description: t('history.kit.stepPairText'),
          action: {
            label: t('history.kit.pairDevice'),
            primary: true,
            onClick: onPairDevice,
          },
        },
        {
          id: 'paste',
          title: t('history.kit.stepPasteTitle'),
          description: shortcut
            ? t('history.kit.stepPasteText', { shortcut: formatShortcutHint(shortcut) })
            : t('history.kit.stepPasteTextUnbound'),
        },
      ]}
    />
  )
}
