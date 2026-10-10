import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SHORTCUT_DEFINITIONS, type ShortcutDefinition } from '@/shortcuts/definitions'
import type { QuickPanelPosition } from '@/types/setting'
import { QuickPanelDoubleTapRow } from './QuickPanelDoubleTapRow'
import { QuickPanelShortcutRow } from './QuickPanelShortcutRow'
import { RestartBanner } from './RestartBanner'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'
import { useShortcutSettings } from './useShortcutSettings'

const log = createLogger('quick-panel-section')

const QUICK_PANEL_SHORTCUT_ID = 'global.toggleQuickPanel'

/**
 * Quick panel feature section.
 *
 * - 开启:`set_quick_panel_enabled` Tauri command 即时注册全局快捷键 +
 *   预创建隐藏面板窗口,无需重启。
 * - 关闭:即时反注册全局快捷键(快捷键立刻失效),但隐藏窗口 / 底层
 *   WKWebView / WebContent XPC 进程不会被销毁——macOS 上销毁路径会触发
 *   崩溃。要彻底释放这些资源,提示用户手动重启 GUI(下次启动按 enabled=false
 *   跳过 pre_create 即可)。
 *
 * 这里把"切换快捷面板"的快捷键也一并展示出来,让用户在同一个 section
 * 内完成"开关 + 配快捷键"两件事;Shortcuts section 里仍然保留同一行,
 * 两处共享同一个 `keyboardShortcuts[global.toggleQuickPanel]` 字段。
 */
export default function QuickPanelSection() {
  const { t } = useTranslation()
  const { setting, updateQuickPanelSetting } = useSetting()

  const { overrides, getCurrentKey, isModified, handleOverrideChange, handleResetShortcut } =
    useShortcutSettings()
  const quickPanelDef = useMemo<ShortcutDefinition | undefined>(
    () => SHORTCUT_DEFINITIONS.find(def => def.id === QUICK_PANEL_SHORTCUT_ID),
    []
  )

  // 用户在本次会话里从开启切到关闭后,显示"重启以彻底释放资源"提示。
  // 不持久化:重启 GUI 后这条 hint 自然消失,因为启动期 pre_create 已经
  // 因 enabled=false 跳过,资源也已释放。再次开启时清掉提示。
  const [disabledThisSession, setDisabledThisSession] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  // Optimistic like every other settings toggle: the switch flips immediately
  // and persists in the background, so it no longer waits a daemon round-trip
  // or dims the group while saving. The session-local "was disabled" hint and
  // the stale-error reset run only after a successful persist.
  const [enabled, setEnabled] = useOptimisticSetting(
    setting?.quickPanel?.enabled ?? false,
    async (next: boolean) => {
      await updateQuickPanelSetting({ enabled: next })
      setDisabledThisSession(!next)
      if (next) setRestartError(null)
    },
    { failureLog: 'Failed to toggle quick panel' }
  )
  const [position, setPosition] = useOptimisticSetting<QuickPanelPosition>(
    setting?.quickPanel?.position ?? 'center',
    next => updateQuickPanelSetting({ position: next }),
    { failureLog: 'Failed to change quick panel position' }
  )
  const restartHintVisible = disabledThisSession && !enabled

  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      // app.restart() 不返回(进程会 exit),后续代码理论上不可达;
      // 走到 catch 说明 spawn 本身就失败了。
      await commands.restartApp()
      return true
    } catch (err) {
      log.error({ err }, '快捷面板关闭后重启应用失败')
      setRestartError(t('settings.restartBanner.errorMessage'))
      return false
    } finally {
      setRestartLoading(false)
    }
  }

  return (
    <div className="flex min-w-0 flex-col gap-8">
      <SettingGroup title={t('settings.sections.quickPanel.featureTitle')}>
        <RestartBanner
          visible={restartHintVisible}
          message={t('settings.sections.quickPanel.restartHint')}
          onRestart={handleRestart}
          loading={restartLoading}
          error={restartError}
          onDismissError={() => setRestartError(null)}
        />
        <SettingRow
          label={t('settings.sections.quickPanel.enable.label')}
          description={t('settings.sections.quickPanel.enable.description')}
        >
          <Switch checked={enabled} onCheckedChange={setEnabled} />
        </SettingRow>
        <SettingRow
          label={t('settings.sections.quickPanel.position.label')}
          description={t('settings.sections.quickPanel.position.description')}
        >
          <Select
            value={position}
            onValueChange={value => setPosition(value as QuickPanelPosition)}
            disabled={!enabled}
          >
            <SelectTrigger className="h-9 w-40 ">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="center">
                {t('settings.sections.quickPanel.position.center')}
              </SelectItem>
              <SelectItem value="follow_cursor">
                {t('settings.sections.quickPanel.position.followCursor')}
              </SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>
      </SettingGroup>

      {quickPanelDef && (
        <SettingGroup title={t('settings.sections.quickPanel.shortcutTitle')}>
          <QuickPanelDoubleTapRow enabled={enabled} />
          <QuickPanelShortcutRow
            definition={quickPanelDef}
            currentKey={getCurrentKey(quickPanelDef)}
            currentOverrides={overrides}
            isModified={isModified(quickPanelDef.id)}
            onOverrideChange={handleOverrideChange}
            onResetShortcut={handleResetShortcut}
          />
        </SettingGroup>
      )}
    </div>
  )
}
