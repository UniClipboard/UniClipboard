import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import type { CongestionController } from '@/types/setting'
import { AllowOverlayAddrsDisclosure } from './AllowOverlayAddrsDisclosure'
import { CustomRelayUrlsField } from './CustomRelayUrlsField'
import { LanOnlyDisclosure } from './LanOnlyDisclosure'
import { RestartBanner } from './RestartBanner'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('network-section')
const SAVE_DELAY_MS = 500
const SAVE_ERROR_DISPLAY_MS = 5000

interface NetworkDraft {
  allowRelayFallback: boolean
  allowOverlayNetworkAddrs: boolean
  customRelayUrls: string[]
  congestionController: CongestionController
}

function normalizeRelayUrls(urls: string[]): string[] {
  return urls.flatMap(url => {
    const trimmed = url.trim()
    return trimmed ? [trimmed] : []
  })
}

function relayUrlListsEqual(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index])
}

function validateRelayUrls(urls: string[]): string | null {
  for (const raw of urls) {
    try {
      const url = new URL(raw)
      if (url.protocol !== 'http:' && url.protocol !== 'https:') return raw
      if (!url.hostname) return raw
    } catch {
      return raw
    }
  }
  return null
}

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
  const persistedCustomRelayUrls = setting?.network?.customRelayUrls ?? []
  const persistedCongestionController: CongestionController =
    setting?.network?.congestionController ?? 'cubic'

  const persistedDraft: NetworkDraft = {
    allowRelayFallback: persistedAllowRelay,
    allowOverlayNetworkAddrs: persistedAllowOverlay,
    customRelayUrls: persistedCustomRelayUrls,
    congestionController: persistedCongestionController,
  }
  const [draftOverride, setDraftOverride] = useState<NetworkDraft | null>(null)
  const draft = draftOverride ?? persistedDraft
  const { allowRelayFallback, allowOverlayNetworkAddrs, customRelayUrls, congestionController } =
    draft

  // pending 状态（来自两个源：用户切换 / PUT 后 restartRequired；不跨 session）
  const [pending, setPending] = useState(false)
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)

  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const saveErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const saveGenerationRef = useRef(0)

  useEffect(() => {
    return () => {
      saveGenerationRef.current += 1
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      if (saveErrorTimerRef.current) clearTimeout(saveErrorTimerRef.current)
    }
  }, [])

  const showSaveError = (message: string) => {
    setSaveError(message)
    if (saveErrorTimerRef.current) clearTimeout(saveErrorTimerRef.current)
    saveErrorTimerRef.current = setTimeout(() => setSaveError(null), SAVE_ERROR_DISPLAY_MS)
  }

  const queueNetworkUpdate = (patch: Partial<NetworkDraft>, showPendingImmediately = true) => {
    const next: NetworkDraft = {
      ...draft,
      ...patch,
    }
    setDraftOverride(next)
    if (showPendingImmediately) setPending(true)
    setSaveError(null)
    setRestartError(null)

    const payload = { ...next, customRelayUrls: normalizeRelayUrls(next.customRelayUrls) }
    const invalidRelayUrl = validateRelayUrls(payload.customRelayUrls)
    if (invalidRelayUrl) {
      // Keep an already queued valid update alive, but ensure its completion
      // cannot clear this newer invalid draft.
      saveGenerationRef.current += 1
      showSaveError(
        t('settings.sections.network.customRelays.invalidUrl', { url: invalidRelayUrl })
      )
      return
    }

    const generation = saveGenerationRef.current + 1
    saveGenerationRef.current = generation
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current)

    const relayChanged = next.allowRelayFallback !== persistedAllowRelay
    const customRelaysChanged = !relayUrlListsEqual(
      payload.customRelayUrls,
      persistedCustomRelayUrls
    )
    saveTimerRef.current = setTimeout(() => {
      saveTimerRef.current = null
      void updateNetworkSetting(payload).then(
        result => {
          if (saveGenerationRef.current !== generation) return
          setDraftOverride(null)
          setPending(result.restartRequired)
        },
        err => {
          if (saveGenerationRef.current !== generation) return
          log.error({ err }, 'Failed to save network settings')
          setDraftOverride(null)
          setPending(false)
          const message = err instanceof Error ? err.message : String(err)
          const errorKey = customRelaysChanged
            ? 'settings.sections.network.customRelays.saveError'
            : relayChanged
              ? 'settings.sections.network.lanOnly.saveError'
              : 'settings.sections.network.allowOverlayAddrs.saveError'
          showSaveError(t(errorKey, { message }))
        }
      )
    }, SAVE_DELAY_MS)
  }

  // ── Switch 切换 handler（LAN-only — 反向命名唯一取反点） ────────
  const handleLanOnlySwitchChange = (checked: boolean) => {
    // FENCE: 反向命名唯一取反点（Pitfall 1 — UI checked = LAN-only ON = allowRelay false）
    const newAllowRelay = !checked
    queueNetworkUpdate({ allowRelayFallback: newAllowRelay })
  }

  const handleCustomRelayUrlsChange = (value: string[]) => {
    queueNetworkUpdate({ customRelayUrls: value }, false)
  }

  // ── Switch 切换 handler（Allow Overlay — 正向同名，不取反） ─────
  const handleAllowOverlaySwitchChange = (checked: boolean) => {
    queueNetworkUpdate({ allowOverlayNetworkAddrs: checked })
  }

  // ── Select 切换 handler（Congestion Controller） ───────────────
  const handleCongestionControllerChange = (value: string) => {
    queueNetworkUpdate({ congestionController: value as CongestionController })
  }

  // ── 「立即重启」按钮 handler ───────────────────────────────────
  const handleRestart = async () => {
    setRestartLoading(true)
    setRestartError(null)
    try {
      await commands.restartDaemon()
      setPending(false)
      return true
    } catch (err) {
      log.error({ err }, 'restart_daemon 失败')
      setRestartError(t('settings.sections.network.restartBanner.errorMessage'))
      return false
    } finally {
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
        message={t('settings.sections.network.restartBanner.message')}
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
      <SettingRow
        label={t('settings.sections.network.congestionController.label')}
        description={t('settings.sections.network.congestionController.description')}
        experimentalKey="network.congestionController"
      >
        <Select value={congestionController} onValueChange={handleCongestionControllerChange}>
          <SelectTrigger
            id="congestion-controller-select"
            size="sm"
            aria-label={t('settings.sections.network.congestionController.label')}
            className="w-[180px]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="cubic">
              CUBIC ({t('settings.sections.network.congestionController.recommended')})
            </SelectItem>
            <SelectItem value="bbr3">BBR3</SelectItem>
          </SelectContent>
        </Select>
      </SettingRow>
      <CustomRelayUrlsField value={customRelayUrls} onChange={handleCustomRelayUrlsChange} />
      {saveError && (
        <div className="px-4 pb-3 text-xs text-destructive" role="alert">
          {saveError}
        </div>
      )}
    </SettingGroup>
  )
}

export default NetworkSection
