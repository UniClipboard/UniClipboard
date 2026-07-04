/**
 * PeerDetailPanel: persistent detail pane for a single paired P2P device.
 *
 * Replaces the former `DeviceSettingsDialog` modal in the devices
 * master-detail layout: the same data flow (member sync preferences via
 * Redux) rendered as an always-visible right-hand panel instead of a
 * stacked dialog. Ledger treatment: hairline separators, micro uppercase
 * labels, a glowing status dot, and hover-quiet secondary actions.
 */

import { AlignLeft, FileIcon, ImageIcon, Link2, Type, type LucideIcon } from 'lucide-react'
import React, { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes } from '@/api/daemon/member'
import { DEFAULT_SEND_CONTENT_TYPES } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import { deriveBadgeKind } from '@/components/device/connection-channel-utils'
import CopyIconButton from '@/components/device/CopyIconButton'
import { contentTypeEntries, getDeviceIcon } from '@/components/device/device-utils'
import PanelFactRow from '@/components/device/PanelFactRow'
import StatusDot from '@/components/device/StatusDot'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  fetchMemberSyncPreferences,
  updateMemberSyncPreferences,
} from '@/store/slices/devicesSlice'

const log = createLogger('peer-detail-panel')

interface PeerDetailPanelProps {
  deviceId: string
  device: SpaceMember | undefined
  globalAutoSyncOff: boolean
  globalFileSyncOff: boolean
  /** Whether LAN-only mode is active (drives the derived channel label). */
  lanOnlyActive: boolean
  onUnpair: (peerId: string) => void
}

const CONTENT_TYPE_ICONS: Partial<Record<keyof ContentTypes, LucideIcon>> = {
  text: Type,
  image: ImageIcon,
  file: FileIcon,
  link: Link2,
  richText: AlignLeft,
}

const PeerDetailPanel: React.FC<PeerDetailPanelProps> = ({
  deviceId,
  device,
  globalAutoSyncOff,
  globalFileSyncOff,
  lanOnlyActive,
  onUnpair,
}) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  const preferences = useAppSelector(state => state.devices.memberSyncPreferences[deviceId])
  const isLoading = useAppSelector(
    state => state.devices.memberSyncPreferencesLoading[deviceId] ?? false
  )

  useEffect(() => {
    if (deviceId) {
      dispatch(fetchMemberSyncPreferences(deviceId))
    }
  }, [dispatch, deviceId])

  // Preferences are updated optimistically in the slice; on failure re-fetch
  // the authoritative value to roll the optimistic change back.
  const reconcileAfterFailure = useCallback(() => {
    dispatch(fetchMemberSyncPreferences(deviceId))
  }, [dispatch, deviceId])

  const handleSendEnabledToggle = useCallback(
    (checked: boolean) => {
      dispatch(updateMemberSyncPreferences({ deviceId, patch: { sendEnabled: checked } }))
        .unwrap()
        .catch(err => {
          log.error({ err }, 'failed to update send-enabled preference')
          reconcileAfterFailure()
        })
    },
    [dispatch, deviceId, reconcileAfterFailure]
  )

  const handleSendContentTypeToggle = useCallback(
    (field: keyof ContentTypes, checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({ deviceId, patch: { sendContentTypes: { [field]: checked } } })
      )
        .unwrap()
        .catch(err => {
          log.error({ err, field }, 'failed to update send content-type preference')
          reconcileAfterFailure()
        })
    },
    [dispatch, deviceId, reconcileAfterFailure]
  )

  const handleRestoreDefaults = useCallback(async () => {
    // UX only exposes the send column, so restore resets send fields only;
    // receive fields keep their current server values.
    try {
      await dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { sendEnabled: true, sendContentTypes: DEFAULT_SEND_CONTENT_TYPES },
        })
      ).unwrap()
      dispatch(fetchMemberSyncPreferences(deviceId))
    } catch (err) {
      log.error({ err }, 'failed to restore default sync preferences')
      reconcileAfterFailure()
    }
  }, [dispatch, deviceId, reconcileAfterFailure])

  if (!deviceId) return null

  const deviceName = device?.deviceName || t('devices.list.labels.unknownDevice')
  const connected = device?.connected ?? false
  const channelKind = deriveBadgeKind(device?.channel ?? 'unknown', lanOnlyActive)
  const channelLabel = t(`devices.list.channel.${channelKind}`)
  const dotTone = !connected ? 'off' : channelKind === 'relay' ? 'warning' : 'success'
  const sendEnabled = preferences?.sendEnabled ?? true
  const sendDisabled = !sendEnabled || globalAutoSyncOff || isLoading
  const showSkeleton = isLoading && !preferences
  const Icon = getDeviceIcon(device?.deviceName)

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-8 py-8">
      {/* ── header ─────────────────────────────────────────────── */}
      <div className="flex items-start gap-4">
        <div
          className={cn(
            'flex size-12 shrink-0 items-center justify-center rounded-xl',
            connected ? 'bg-success/10 text-success' : 'bg-muted text-muted-foreground'
          )}
        >
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
          <Icon className="size-6" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-xl font-semibold tracking-tight text-foreground">
            {deviceName}
          </h3>
          <p className="mt-1.5 flex items-center gap-2 text-xs">
            <StatusDot tone={dotTone} />
            <span
              className={cn(
                'font-medium',
                connected
                  ? channelKind === 'relay'
                    ? 'text-warning'
                    : 'text-success'
                  : 'text-muted-foreground'
              )}
            >
              {connected ? t('devices.list.status.online') : t('devices.list.status.offline')}
            </span>
            <span className="text-muted-foreground/50">·</span>
            <span className="text-muted-foreground">{channelLabel}</span>
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="shrink-0 border-destructive/30 text-destructive hover:border-destructive/50 hover:bg-destructive/10 hover:text-destructive"
          onClick={() => onUnpair(deviceId)}
        >
          {t('devices.list.actions.unpair')}
        </Button>
      </div>

      {/* ── connection facts (same row layout as LocalDevicePanel) ── */}
      <div className="flex flex-col border-y border-border/50">
        <PanelFactRow label={t('devices.panel.fields.peerId')}>
          <span className="truncate font-mono text-xs font-medium" title={device?.peerId}>
            {device?.peerId ?? deviceId}
          </span>
          <CopyIconButton value={device?.peerId ?? deviceId} />
        </PanelFactRow>
        <PanelFactRow label={t('devices.settings.fields.channel')}>
          <span className="text-xs font-medium">{channelLabel}</span>
        </PanelFactRow>
        {device?.connectionAddress && (
          <PanelFactRow label={t('devices.settings.fields.address')}>
            <span className="truncate font-mono text-xs font-medium">
              {device.connectionAddress}
            </span>
          </PanelFactRow>
        )}
      </div>

      {/* ── sync settings ──────────────────────────────────────── */}
      <section>
        <div className="flex items-center justify-between pb-1">
          <h4 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
            {t('devices.settings.sync.title')}
          </h4>
          <button
            type="button"
            onClick={handleRestoreDefaults}
            disabled={globalAutoSyncOff || isLoading}
            className="text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t('devices.settings.sync.restoreDefaults')}
          </button>
        </div>

        {showSkeleton ? (
          <SyncSettingsSkeleton />
        ) : (
          <>
            <div className="flex items-start justify-between gap-3 border-b border-border/40 py-3.5">
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-foreground">
                  {t('devices.settings.sync.rules.sendEnabled.title')}
                </p>
                <p className="mt-0.5 text-[11px] leading-snug text-muted-foreground">
                  {t('devices.settings.sync.rules.sendEnabled.description')}
                </p>
              </div>
              <Switch
                checked={sendEnabled}
                onCheckedChange={handleSendEnabledToggle}
                disabled={globalAutoSyncOff || isLoading}
              />
            </div>

            <div className="pt-4">
              <p className="pb-2 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
                {t('devices.settings.sync.contentTypes')}
              </p>
              <div
                className={cn(
                  'grid grid-cols-2 gap-2 transition-opacity',
                  sendDisabled && 'pointer-events-none opacity-50'
                )}
              >
                {CONTENT_TYPE_FIELDS.map(({ field, i18nKey, status }) => {
                  const isComingSoon = status === 'coming_soon'
                  const isGlobalFileSyncDisabled = field === 'file' && globalFileSyncOff
                  const disabled = isComingSoon || isGlobalFileSyncDisabled || sendDisabled

                  let suffix: React.ReactNode = null
                  if (isComingSoon) {
                    suffix = (
                      <Badge variant="secondary" className="px-1.5 py-0 text-[9px]">
                        {t('devices.settings.badges.comingSoon')}
                      </Badge>
                    )
                  } else if (isGlobalFileSyncDisabled) {
                    suffix = (
                      <Badge
                        variant="outline"
                        className="border-warning/20 bg-warning/10 px-1.5 py-0 text-[9px] text-warning"
                      >
                        {t('devices.settings.badges.globalFileSyncOff')}
                      </Badge>
                    )
                  }

                  return (
                    <ContentTypeToggle
                      key={field}
                      // `CONTENT_TYPE_FIELDS` excludes codeSnippet, so `field`
                      // only hits the 5 mapped keys; assert past the Partial
                      // index signature.
                      icon={CONTENT_TYPE_ICONS[field]!}
                      label={t(`devices.settings.sync.rules.${i18nKey}.title`)}
                      checked={preferences?.sendContentTypes[field] ?? true}
                      onChange={checked => handleSendContentTypeToggle(field, checked)}
                      disabled={disabled}
                      suffix={suffix}
                    />
                  )
                })}
              </div>
            </div>
          </>
        )}
      </section>
    </div>
  )
}

export default PeerDetailPanel

// ────────────────────────────────────────────────────────────────
// Local helpers (file-private)
// ────────────────────────────────────────────────────────────────

const CONTENT_TYPE_FIELDS = contentTypeEntries

const ContentTypeToggle: React.FC<{
  icon: LucideIcon
  label: string
  checked: boolean
  disabled?: boolean
  suffix?: React.ReactNode
  onChange: (v: boolean) => void
}> = ({ icon: Icon, label, checked, disabled, suffix, onChange }) => (
  <label
    className={cn(
      'flex items-center gap-2 rounded-lg border px-3 py-2 transition-colors',
      disabled
        ? 'cursor-not-allowed border-border/60 bg-card/30'
        : checked
          ? 'cursor-pointer border-primary/40 bg-primary/5'
          : 'cursor-pointer border-border/60 bg-card/50 hover:bg-muted/30'
    )}
  >
    <Icon
      className={cn(
        'size-3.5 shrink-0',
        checked && !disabled ? 'text-primary' : 'text-muted-foreground'
      )}
    />
    <span className="flex-1 truncate text-xs font-medium text-foreground">{label}</span>
    {suffix}
    <Switch size="sm" checked={checked} onCheckedChange={onChange} disabled={disabled} />
  </label>
)

const SyncSettingsSkeleton: React.FC = () => (
  <div className="space-y-3 pt-2">
    <Skeleton className="h-[58px] w-full rounded-lg" />
    <div className="space-y-2">
      <Skeleton className="h-3 w-16 rounded-md" />
      <div className="grid grid-cols-2 gap-2">
        {[0, 1, 2, 3].map(i => (
          <Skeleton key={i} className="h-[34px] rounded-lg" />
        ))}
      </div>
    </div>
  </div>
)
