import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CustomRelayMutationError } from '@/api/daemon'
import { AllowOverlayAddrsDisclosure } from '@/components/setting/AllowOverlayAddrsDisclosure'
import { BuiltInRelaysField } from '@/components/setting/BuiltInRelaysField'
import { CustomRelayUrlsField } from '@/components/setting/CustomRelayUrlsField'
import { LanOnlyDisclosure } from '@/components/setting/LanOnlyDisclosure'
import { RestartBanner } from '@/components/setting/RestartBanner'
import { SettingSelect, SettingSwitch } from '@/components/setting/SettingControls'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'
import { useRelayOverview } from '@/components/setting/useRelayOverview'
import { Button } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import {
  awaitNetworkSettingsSaves,
  networkDaemonRestarted,
  trackNetworkSettingsSave,
  useNetworkRestartRequired,
} from '@/store/networkRestartStore'
import type { CongestionController, CustomRelayMutation } from '@/types/setting'

const log = createLogger('network-section')
const SAVE_DELAY_MS = 500
const SAVE_ERROR_DISPLAY_MS = 5000

interface NetworkDraft {
  allowRelayFallback: boolean
  allowOverlayNetworkAddrs: boolean
  congestionController: CongestionController
}

class LocalizedRelayMutationError extends Error {
  readonly cause: unknown
  readonly presented = true

  constructor(message: string, cause: unknown) {
    super(message)
    this.name = 'LocalizedRelayMutationError'
    this.cause = cause
  }
}

/** Debounced network drafts are saved before navigation or daemon restart. */
const NetworkSection: React.FC = () => {
  const { t } = useTranslation()
  const {
    setting,
    error,
    customRelays,
    relayLoading,
    relayError,
    reloadCustomRelays,
    mutateCustomRelay,
    updateNetworkSetting,
  } = useSetting()

  // 当前持久值（来自 SettingContext，作为 baseline）
  const persistedAllowRelay = setting?.network?.allowRelayFallback ?? true
  // Engine owns the relay overview; re-read it after any relay or LAN-only change.
  const relayOverview = useRelayOverview(customRelays, persistedAllowRelay, !relayLoading)
  const persistedAllowOverlay = setting?.network?.allowOverlayNetworkAddrs ?? false
  const persistedCongestionController: CongestionController =
    setting?.network?.congestionController ?? 'cubic'

  const persistedDraft: NetworkDraft = {
    allowRelayFallback: persistedAllowRelay,
    allowOverlayNetworkAddrs: persistedAllowOverlay,
    congestionController: persistedCongestionController,
  }
  const [draftOverride, setDraftOverride] = useState<NetworkDraft | null>(null)
  const draft = draftOverride ?? persistedDraft
  const { allowRelayFallback, allowOverlayNetworkAddrs, congestionController } = draft

  const pending = useNetworkRestartRequired()
  const [restartLoading, setRestartLoading] = useState(false)
  const [restartError, setRestartError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)

  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const saveErrorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const saveGenerationRef = useRef(0)
  const pendingNetworkPatchRef = useRef<Partial<NetworkDraft>>({})
  const saveInFlightRef = useRef<Promise<void> | null>(null)

  const showSaveError = (message: string) => {
    setSaveError(message)
    if (saveErrorTimerRef.current) clearTimeout(saveErrorTimerRef.current)
    saveErrorTimerRef.current = setTimeout(() => setSaveError(null), SAVE_ERROR_DISPLAY_MS)
  }

  const flushNetworkUpdate = (): Promise<void> => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
    saveTimerRef.current = null
    const payload = pendingNetworkPatchRef.current
    if (Object.keys(payload).length === 0) return saveInFlightRef.current ?? Promise.resolve()
    pendingNetworkPatchRef.current = {}
    const generation = saveGenerationRef.current
    const relayChanged =
      payload.allowRelayFallback !== undefined && payload.allowRelayFallback !== persistedAllowRelay
    const save = trackNetworkSettingsSave(
      (saveInFlightRef.current ?? Promise.resolve())
        .catch(() => {})
        .then(() => updateNetworkSetting(payload))
    ).then(
      () => {
        if (saveGenerationRef.current === generation) setDraftOverride(null)
      },
      err => {
        log.error({ err }, 'Failed to save network settings')
        if (saveGenerationRef.current === generation) {
          setDraftOverride(null)
          const message = err instanceof Error ? err.message : String(err)
          const errorKey = relayChanged
            ? 'settings.sections.network.lanOnly.saveError'
            : 'settings.sections.network.allowOverlayAddrs.saveError'
          showSaveError(t(errorKey, { message }))
        }
        throw err
      }
    )
    saveInFlightRef.current = save
    return save
  }
  const flushOnUnmount = useEffectEvent(() => {
    // The store keeps this write awaitable after the page has unmounted.
    void flushNetworkUpdate().catch(() => {})
    saveGenerationRef.current += 1
  })
  useEffect(() => {
    return () => {
      flushOnUnmount()
      if (saveErrorTimerRef.current) clearTimeout(saveErrorTimerRef.current)
    }
  }, [])

  const queueNetworkUpdate = (patch: Partial<NetworkDraft>) => {
    setDraftOverride({ ...draft, ...patch })
    setSaveError(null)
    setRestartError(null)
    pendingNetworkPatchRef.current = { ...pendingNetworkPatchRef.current, ...patch }
    saveGenerationRef.current += 1
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
    saveTimerRef.current = setTimeout(() => {
      void flushNetworkUpdate().catch(() => {})
    }, SAVE_DELAY_MS)
  }

  // ── Switch 切换 handler（LAN-only — 反向命名唯一取反点） ────────
  const handleLanOnlySwitchChange = (checked: boolean) => {
    // FENCE: 反向命名唯一取反点（Pitfall 1 — UI checked = LAN-only ON = allowRelay false）
    const newAllowRelay = !checked
    queueNetworkUpdate({ allowRelayFallback: newAllowRelay })
  }

  const saveCustomRelay = async (mutation: CustomRelayMutation) => {
    const trimmedMutation =
      mutation.action === 'delete' ? mutation : { ...mutation, url: mutation.url.trim() }
    setSaveError(null)
    setRestartError(null)
    try {
      const result = await trackNetworkSettingsSave(mutateCustomRelay(trimmedMutation))
      return result
    } catch (err) {
      log.error({ err }, 'Failed to save a custom relay')
      if (err instanceof CustomRelayMutationError) {
        const { url } = mutation
        const key =
          err.kind === 'invalidUrl'
            ? 'settings.sections.network.customRelays.invalidUrl'
            : err.kind === 'duplicate'
              ? 'settings.sections.network.customRelays.duplicateUrl'
              : err.kind === 'notFound'
                ? 'settings.sections.network.customRelays.notFound'
                : 'settings.sections.network.customRelays.deleteCredentialAfterAddress'
        const localized = new LocalizedRelayMutationError(t(key, { url }), err)
        showSaveError(localized.message)
        throw localized
      }
      throw err
    }
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
      await flushNetworkUpdate()
      await awaitNetworkSettingsSaves()
      await commands.restartDaemon()
      networkDaemonRestarted()
      await relayOverview.reload()
      return true
    } catch (err) {
      log.error({ err }, 'Failed to save settings or restart daemon')
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
    <>
      <div className="overflow-hidden rounded-lg border border-border/60 empty:hidden">
        <RestartBanner
          visible={
            pending || draftOverride !== null || relayOverview.overview?.changePending === true
          }
          message={t('settings.sections.network.restartBanner.message')}
          onRestart={handleRestart}
          loading={restartLoading}
          error={restartError}
          onDismissError={() => setRestartError(null)}
        />
      </div>
      <fieldset disabled={restartLoading} className="contents">
        {saveError && (
          <div
            className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-ui-body text-destructive"
            role="alert"
          >
            {saveError}
          </div>
        )}
        <SettingGroup title={t('settings.sections.network.groups.connection')}>
          <SettingRow
            label={t('settings.sections.network.lanOnly.label')}
            labelExtra={<LanOnlyDisclosure />}
            description={t('settings.sections.network.lanOnly.description')}
            experimentalKey="network.lanOnly"
          >
            <SettingSwitch
              id="lan-only-switch"
              label={t('settings.sections.network.lanOnly.label')}
              // FENCE: LAN-only ON means allowRelayFallback=false.
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
            <SettingSwitch
              id="allow-overlay-addrs-switch"
              label={t('settings.sections.network.allowOverlayAddrs.label')}
              checked={allowOverlayNetworkAddrs}
              onCheckedChange={handleAllowOverlaySwitchChange}
            />
          </SettingRow>
        </SettingGroup>
        <BuiltInRelaysField
          overview={relayOverview.overview}
          loading={relayOverview.loading}
          failed={relayOverview.failed}
          onRetry={() => void relayOverview.reload()}
        />
        {relayError ? (
          <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-ui-body text-destructive">
            <p>{t('settings.sections.network.customRelays.loadError')}</p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-2"
              disabled={relayLoading}
              onClick={() => {
                void reloadCustomRelays().catch(err => {
                  log.error({ err }, 'Failed to retry loading custom relays')
                })
              }}
            >
              {t('settings.sections.network.customRelays.retry')}
            </Button>
          </div>
        ) : relayLoading ? (
          <p className="py-4 text-ui-body text-muted-foreground">
            {t('settings.sections.network.customRelays.loading')}
          </p>
        ) : (
          <CustomRelayUrlsField value={customRelays} onSave={saveCustomRelay} />
        )}
        <SettingGroup title={t('settings.sections.network.groups.performance')}>
          <SettingRow
            label={t('settings.sections.network.congestionController.label')}
            description={t('settings.sections.network.congestionController.description')}
            experimentalKey="network.congestionController"
          >
            <SettingSelect
              label={t('settings.sections.network.congestionController.label')}
              value={congestionController}
              onValueChange={handleCongestionControllerChange}
              items={[
                {
                  value: 'cubic',
                  label: `CUBIC (${t('settings.sections.network.congestionController.recommended')})`,
                },
                { value: 'bbr3', label: 'BBR3' },
              ]}
            />
          </SettingRow>
        </SettingGroup>
      </fieldset>
    </>
  )
}

export default NetworkSection
