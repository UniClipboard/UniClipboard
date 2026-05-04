import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { LanOnlyDisclosure } from './LanOnlyDisclosure'
import { RestartBanner } from './RestartBanner'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { Switch } from '@/components/ui'
import { useDebounce } from '@/hooks/useDebounce'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { invokeWithTrace } from '@/lib/tauri-command'

const log = createLogger('network-section')

/** Wire shape returned by `get_restart_state` Tauri command (Plan 02). */
interface RestartState {
  processStartedAt: number
  settingsMtime: number
}

/**
 * NetworkSection — Phase 95.
 *
 * 用户在 Settings → Network 切换 LAN-only Mode；切换后看到持久 inline RestartBanner，
 * 点「立即重启」触发 Tauri app.restart()。Pending 跨 session 由 Tauri command
 * `get_restart_state` 推导（settingsMtime > processStartedAt ⇒ pending — D-D1）。
 *
 * # 反向命名规则（per ROADMAP 核心约束 + Pitfall 1）
 * UI checked === ON === LAN-only === allowRelayFallback === false。
 * 本组件含**唯一一处**前端取反点：`<Switch checked={!allowRelayFallback}>` 与
 * onCheckedChange 内的 `setAllowRelayFallback(!checked)`。
 */
const NetworkSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateNetworkSetting } = useSetting()

  // 当前持久值（来自 SettingContext，作为 baseline）
  const persistedAllowRelay = setting?.network?.allowRelayFallback ?? true

  // 本地乐观 state（D-D2：切换后立即更新，不等 PUT 返回）
  const [allowRelayFallback, setAllowRelayFallback] = useState(persistedAllowRelay)

  // pending 状态（来自三个源：用户切换 / mtime 推导 / Tauri 重启失败回滚）
  const [pending, setPending] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)

  // 防止 useEffect 在 mount 时立即触发 PUT（pristine flag）
  const isPristineRef = useRef(true)

  // debounced 写盘值（D-D3：500ms after last user change）
  const debouncedAllowRelay = useDebounce(allowRelayFallback, 500)

  // ── Effect 1: setting 加载后同步本地 state baseline ─────────────
  useEffect(() => {
    if (setting?.network) {
      setAllowRelayFallback(setting.network.allowRelayFallback)
    }
  }, [setting])

  // ── Effect 2: mount 时调 get_restart_state 推导跨-session pending ──
  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const state = await invokeWithTrace<RestartState>('get_restart_state')
        if (cancelled) return
        // D-D1: settingsMtime > processStartedAt ⇒ 本进程启动后 settings.json 改过
        // ⇒ 还没 relaunch ⇒ pending
        if (state.settingsMtime > state.processStartedAt && state.processStartedAt > 0) {
          setPending(true)
        }
      } catch (err) {
        log.error({ err }, 'Failed to query restart state')
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  // ── Effect 3: debounced PUT /settings ───────────────────────────
  useEffect(() => {
    if (isPristineRef.current) {
      isPristineRef.current = false
      return
    }
    if (!setting) return
    // 用户实际改变了值才发 PUT
    if (debouncedAllowRelay === persistedAllowRelay) return

    void (async () => {
      try {
        const result = await updateNetworkSetting({
          allowRelayFallback: debouncedAllowRelay,
        })
        if (result.restartRequired) {
          setPending(true)
        }
      } catch (err) {
        log.error({ err }, '保存 LAN-only 设置失败')
        // PUT 失败回滚 Switch 视觉到 persisted 值 + 显示 inline saveError
        setAllowRelayFallback(persistedAllowRelay)
        setPending(false)
        const message = err instanceof Error ? err.message : String(err)
        setSaveError(t('settings.sections.network.lanOnly.saveError', { message }))
        // 5s 后自动清除 saveError（per UI-SPEC interaction contract）
        window.setTimeout(() => setSaveError(null), 5000)
      }
    })()
  }, [debouncedAllowRelay])

  // ── Switch 切换 handler（反向命名唯一取反点） ──────────────────
  const handleSwitchChange = (checked: boolean) => {
    const newAllowRelay = !checked // ← 唯一取反点
    setAllowRelayFallback(newAllowRelay)
    setPending(true)
    setSaveError(null)
    setRestartError(null)
  }

  // ── 「立即重启」按钮 handler ───────────────────────────────────
  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      await invokeWithTrace<void>('restart_app')
      // app.restart() 不返回；以下不可达
    } catch (err) {
      log.error({ err }, 'app.restart() 失败')
      setRestartError(t('settings.sections.network.restartBanner.errorMessage'))
      setRestartLoading(false)
    }
  }

  // ── error state（getSettings 失败）─────────────────────────────
  if (error) {
    return (
      <div className="text-destructive py-4">
        {t('settings.sections.sync.loadError')} {error}
      </div>
    )
  }

  return (
    <SettingGroup title={t('settings.categories.network')}>
      <RestartBanner
        visible={pending}
        onRestart={handleRestart}
        loading={restartLoading}
        error={restartError}
        onDismissError={() => setRestartError(null)}
      />
      <SettingRow
        label={t('settings.sections.network.lanOnly.label')}
        labelExtra={<LanOnlyDisclosure />}
        description={t('settings.sections.network.lanOnly.description')}
      >
        <Switch
          id="lan-only-switch"
          checked={!allowRelayFallback}
          onCheckedChange={handleSwitchChange}
        />
      </SettingRow>
      {saveError && (
        <div className="px-4 pb-3 text-xs text-destructive" role="alert">
          {saveError}
        </div>
      )}
    </SettingGroup>
  )
}

export default NetworkSection
