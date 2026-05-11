import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AllowOverlayAddrsDisclosure } from './AllowOverlayAddrsDisclosure'
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

/**
 * NetworkSection — Phase 95.
 *
 * 用户在 Settings → Network 切换 LAN-only Mode；切换后看到持久 inline RestartBanner，
 * 点「立即重启」触发 Tauri app.restart()。Pending 仅 in-memory（用户当前 session 内
 * 切换后显示），不跨 session 持久化 —— 关掉 Settings 面板后状态会重置，避免基于
 * settings.json mtime 的跨 session 推导造成误报（mtime 无法区分到底改了哪个字段）。
 *
 * # Pitfall 防御 audit（Phase 95 PLAN.md Task 3 fence）
 * - **Pitfall 1（反向命名）**：UI checked === ON === LAN-only === allowRelayFallback === false。
 *   本组件含**唯一一处**前端取反点（line marker `// FENCE: 反向命名唯一取反点` 标注两处）。
 *   全工程 grep `!allowRelayFallback` 仅命中 NetworkSection.tsx 与本组件单元测试 — 其它文件 0 匹配。
 *   `allowOverlayNetworkAddrs` 为正向同名字段（UI checked === 字段值），不参与取反铁律。
 * - **Pitfall 5（边界透明）**：禁词清单 `fully offline / 完全离线 / 绝对私有 / no internet /
 *   private mode / encrypted-and-local` 全工程 0 匹配；4 类外网请求由 LanOnlyDisclosure 显式披露。
 * - **Pitfall 10（重启 UX 半生效）**：使用持久 inline RestartBanner（不是 toast 也不是 sonner）；
 *   debounce 500ms 防 disk I/O 爆；切换瞬间 setPending(true) 乐观显示，不等 PUT 返回。
 * - **Pitfall 11（占位组件残留）**：旧 `Network settings are not yet available` /
 *   `网络设置功能在新架构中尚未实现` / `settings.sections.network.placeholder` 全部清零。
 *
 * # 共享 RestartBanner
 *   两个开关（LAN-only / Allow Overlay Addrs）任一改动后都使 daemon 需要重启
 *   （iroh endpoint bind-time 常量 + BIND_LOCK 进程级单次 bind）。pending 状态合并，
 *   一个 banner 服务两个开关。
 */
const NetworkSection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateNetworkSetting } = useSetting()

  // 当前持久值（来自 SettingContext，作为 baseline）
  const persistedAllowRelay = setting?.network?.allowRelayFallback ?? true
  const persistedAllowOverlay = setting?.network?.allowOverlayNetworkAddrs ?? false

  // 本地乐观 state（D-D2：切换后立即更新，不等 PUT 返回）
  const [allowRelayFallback, setAllowRelayFallback] = useState(persistedAllowRelay)
  const [allowOverlayNetworkAddrs, setAllowOverlayNetworkAddrs] = useState(persistedAllowOverlay)

  // pending 状态（来自两个源：用户切换 / PUT 后 restartRequired；不跨 session）
  const [pending, setPending] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)

  // 防止 useEffect 在 mount 时立即触发 PUT（网络设置整体只跳过一次）
  const isPristineRef = useRef(true)

  // debounced 写盘值（D-D3：500ms after last user change）
  const networkDraft = useMemo(
    () => ({
      allowRelayFallback,
      allowOverlayNetworkAddrs,
    }),
    [allowRelayFallback, allowOverlayNetworkAddrs]
  )
  const debouncedNetwork = useDebounce(networkDraft, 500)

  // ── Effect 1: setting 加载后同步本地 state baseline ─────────────
  useEffect(() => {
    if (setting?.network) {
      setAllowRelayFallback(setting.network.allowRelayFallback)
      setAllowOverlayNetworkAddrs(setting.network.allowOverlayNetworkAddrs)
    }
  }, [setting])

  // ── Effect 2: debounced PUT for the network settings group ──────
  useEffect(() => {
    if (isPristineRef.current) {
      isPristineRef.current = false
      return
    }
    if (!setting) return
    const relayChanged = debouncedNetwork.allowRelayFallback !== persistedAllowRelay
    const overlayChanged = debouncedNetwork.allowOverlayNetworkAddrs !== persistedAllowOverlay
    if (!relayChanged && !overlayChanged) return

    void (async () => {
      try {
        const result = await updateNetworkSetting(debouncedNetwork)
        if (result.restartRequired) {
          setPending(true)
        } else {
          setPending(false)
        }
      } catch (err) {
        log.error({ err }, '保存网络设置失败')
        setAllowRelayFallback(persistedAllowRelay)
        setAllowOverlayNetworkAddrs(persistedAllowOverlay)
        setPending(false)
        const message = err instanceof Error ? err.message : String(err)
        const errorKey = relayChanged
          ? 'settings.sections.network.lanOnly.saveError'
          : 'settings.sections.network.allowOverlayAddrs.saveError'
        setSaveError(t(errorKey, { message }))
        window.setTimeout(() => setSaveError(null), 5000)
      }
    })()
  }, [debouncedNetwork])

  // ── Switch 切换 handler（LAN-only — 反向命名唯一取反点） ────────
  const handleLanOnlySwitchChange = (checked: boolean) => {
    // FENCE: 反向命名唯一取反点（Pitfall 1 — UI checked = LAN-only ON = allowRelay false）
    const newAllowRelay = !checked
    setAllowRelayFallback(newAllowRelay)
    setPending(true)
    setSaveError(null)
    setRestartError(null)
  }

  // ── Switch 切换 handler（Allow Overlay — 正向同名，不取反） ─────
  const handleAllowOverlaySwitchChange = (checked: boolean) => {
    setAllowOverlayNetworkAddrs(checked)
    setPending(true)
    setSaveError(null)
    setRestartError(null)
  }

  // ── 「立即重启」按钮 handler ───────────────────────────────────
  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      // 走进程级重启 —— iroh `IrohNodeBuilder::bind` 是进程级单次约束
      // (Pitfall 3),LAN-only Mode 切换改 iroh_config 必须新进程重新 bind。
      // app.restart() 不返回(进程会 exit),所以后续代码理论上不可达。
      await invokeWithTrace<void>('restart_app')
    } catch (err) {
      log.error({ err }, 'restart_app 失败')
      setRestartError(t('settings.sections.network.restartBanner.errorMessage'))
      setRestartLoading(false)
    }
  }

  // ── error state（getSettings 失败）─────────────────────────────
  if (error) {
    return (
      <div className="text-destructive py-4">
        {t('settings.sections.network.loadError')} {error}
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
        experimentalKey="network.lanOnly"
      >
        <Switch
          id="lan-only-switch"
          aria-label={t('settings.sections.network.lanOnly.label')}
          // FENCE: 反向命名唯一取反点（Pitfall 1 — checked=ON ⇔ allowRelayFallback=false）
          checked={!allowRelayFallback}
          onCheckedChange={handleLanOnlySwitchChange}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.sections.network.allowOverlayAddrs.label')}
        labelExtra={<AllowOverlayAddrsDisclosure />}
        description={t('settings.sections.network.allowOverlayAddrs.description')}
        experimentalKey="network.allowOverlayAddrs"
      >
        <Switch
          id="allow-overlay-addrs-switch"
          aria-label={t('settings.sections.network.allowOverlayAddrs.label')}
          checked={allowOverlayNetworkAddrs}
          onCheckedChange={handleAllowOverlaySwitchChange}
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
