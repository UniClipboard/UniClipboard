/**
 * PeerDetailPanel: persistent detail pane for a single paired P2P device.
 *
 * Replaces the former `DeviceSettingsDialog` modal in the devices
 * master-detail layout: the same data flow (member sync preferences via
 * Redux) rendered as an always-visible right-hand panel instead of a
 * stacked dialog. Ledger treatment: hairline separators, micro uppercase
 * labels, a glowing status dot, and hover-quiet secondary actions.
 */

import { CircleHelp, RotateCcw, Unlink } from 'lucide-react'
import React, { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes, MemberProtectionStatus } from '@/api/daemon/member'
import { DEFAULT_SEND_CONTENT_TYPES } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import { deriveBadgeKind, derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import CopyIconButton from '@/components/device/CopyIconButton'
import { getDeviceIcon } from '@/components/device/device-utils'
import PanelFactRow from '@/components/device/PanelFactRow'
import StatusDot from '@/components/device/StatusDot'
import SyncSettingsTable from '@/components/device/SyncSettingsTable'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { toast } from '@/components/ui/toast'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
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
  const protectionStatus = useAppSelector(
    state =>
      state.devices.spaceProtection?.members.find(member => member.deviceId === deviceId)?.status
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

  const handlePreferenceUpdateFailure = useCallback(
    (err: unknown, message: string, fields: Record<string, unknown> = {}) => {
      log.error({ err, ...fields }, message)
      toast.error(t('devices.settings.sync.updateFailed'))
      reconcileAfterFailure()
    },
    [reconcileAfterFailure, t]
  )

  const handleSendEnabledToggle = useCallback(
    (checked: boolean) => {
      dispatch(updateMemberSyncPreferences({ deviceId, patch: { sendEnabled: checked } }))
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update send-enabled preference')
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure]
  )

  const handleSendContentTypeToggle = useCallback(
    (field: keyof ContentTypes, checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({ deviceId, patch: { sendContentTypes: { [field]: checked } } })
      )
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update send content-type preference', {
            field,
          })
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure]
  )

  const handleReceiveEnabledToggle = useCallback(
    (checked: boolean) => {
      dispatch(updateMemberSyncPreferences({ deviceId, patch: { receiveEnabled: checked } }))
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update receive-enabled preference')
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure]
  )

  const handleReceiveContentTypeToggle = useCallback(
    (field: keyof ContentTypes, checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { receiveContentTypes: { [field]: checked } },
        })
      )
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update receive content-type preference', {
            field,
          })
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure]
  )

  const handleRestoreDefaults = useCallback(async () => {
    try {
      await dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: {
            sendEnabled: true,
            receiveEnabled: true,
            sendContentTypes: DEFAULT_SEND_CONTENT_TYPES,
            receiveContentTypes: DEFAULT_SEND_CONTENT_TYPES,
          },
        })
      ).unwrap()
      dispatch(fetchMemberSyncPreferences(deviceId))
    } catch (err) {
      handlePreferenceUpdateFailure(err, 'failed to restore default sync preferences')
    }
  }, [dispatch, deviceId, handlePreferenceUpdateFailure])

  if (!deviceId) return null

  const deviceName = device?.deviceName || t('devices.list.labels.unknownDevice')
  const connected = device?.connected ?? false
  const channelKind = deriveBadgeKind(device?.channel ?? 'unknown', lanOnlyActive)
  const channelLabel = t(`devices.list.channel.${channelKind}`)
  const dotTone = derivePeerStatusTone(device?.channel ?? 'unknown', connected)
  const sendEnabled = preferences?.sendEnabled ?? true
  const receiveEnabled = preferences?.receiveEnabled ?? true
  const showSkeleton = isLoading && !preferences
  const Icon = getDeviceIcon(device?.deviceName)

  return (
    <div className="@container flex min-h-full w-full flex-col bg-background">
      <header className="flex items-center gap-3 border-b border-border/40 bg-card/40 px-5 py-4 @md:px-6">
        <div
          className={cn(
            'flex size-10 shrink-0 items-center justify-center rounded-lg border',
            connected ? 'bg-success/10 text-success' : 'bg-muted text-muted-foreground'
          )}
        >
          {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
          <Icon className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-lg font-semibold text-foreground">{deviceName}</h3>
          <p className="mt-1 flex items-center gap-2 text-[11px]">
            <StatusDot tone={dotTone} />
            <span
              className={cn(
                'font-medium',
                connected && device?.channel === 'relay'
                  ? 'text-info'
                  : connected && device?.channel === 'direct'
                    ? 'text-success'
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
          variant="ghost"
          size="sm"
          aria-label={t('devices.list.actions.unpair')}
          title={t('devices.list.actions.unpair')}
          className="shrink-0 text-destructive hover:bg-destructive/10 hover:text-destructive"
          onClick={() => onUnpair(deviceId)}
        >
          <Unlink className="size-3.5" />
          <span className="hidden @sm:inline">{t('devices.list.actions.unpair')}</span>
        </Button>
      </header>

      <div className="grid content-start @3xl:flex-1 @3xl:content-stretch @3xl:grid-cols-[minmax(16rem,0.82fr)_minmax(24rem,1.18fr)]">
        <section className="px-5 pt-5 pb-3 @md:px-6 @3xl:py-5">
          <h4 className="text-xs font-semibold text-foreground/80">
            {t('devices.settings.sections.connection')}
          </h4>
          <div className="mt-2.5 flex flex-col gap-0.5 [&>div]:border-0 [&>div]:py-2">
            <PanelFactRow label={t('devices.panel.fields.peerId')}>
              <span className="inline-flex min-w-0 max-w-full items-center gap-1">
                <span
                  className="min-w-0 truncate font-mono text-xs font-medium"
                  title={device?.peerId}
                >
                  {device?.peerId ?? deviceId}
                </span>
                <CopyIconButton value={device?.peerId ?? deviceId} />
              </span>
            </PanelFactRow>
            <PanelFactRow label={t('devices.settings.fields.channel')}>
              <span className="text-xs font-medium">{channelLabel}</span>
            </PanelFactRow>
            {protectionStatus && protectionStatus !== 'protected' && (
              <PanelFactRow label={t('devices.protection.label')}>
                <Badge variant="outline" className={protectionBadgeClass(protectionStatus)}>
                  {t(`devices.protection.status.${protectionStatus}`)}
                </Badge>
                {protectionStatus === 'awaiting_readmission' && (
                  <TooltipProvider delay={200}>
                    <Tooltip>
                      <TooltipTrigger
                        render={
                          <button
                            type="button"
                            aria-label={t('devices.protection.upgradePeerHelp')}
                            className="flex size-5 shrink-0 items-center justify-center rounded text-warning/80 transition-colors hover:bg-warning/10 hover:text-warning focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                          />
                        }
                      >
                        <CircleHelp className="size-3.5" />
                      </TooltipTrigger>
                      <TooltipContent
                        side="top"
                        sideOffset={6}
                        className="max-w-64 text-left leading-relaxed whitespace-normal"
                      >
                        {t('devices.protection.upgradePeerHelp')}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                )}
              </PanelFactRow>
            )}
            {device?.connectionAddress && (
              <PanelFactRow label={t('devices.settings.fields.address')}>
                <span className="truncate font-mono text-xs font-medium">
                  {device.connectionAddress}
                </span>
              </PanelFactRow>
            )}
          </div>
        </section>

        <section className="px-5 pt-3 pb-5 @md:px-6 @3xl:py-5">
          <div className="flex h-7 items-center justify-between">
            <div className="flex items-center gap-1.5">
              <h4 className="text-[13px] font-semibold text-foreground/90">
                {t('devices.settings.sync.title')}
              </h4>
              <TooltipProvider delay={250}>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <button
                        type="button"
                        aria-label={t('devices.settings.sync.helpLabel')}
                        className="flex size-5 items-center justify-center rounded text-muted-foreground/65 transition-colors hover:bg-muted/40 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                      />
                    }
                  >
                    <CircleHelp className="size-3.5" />
                  </TooltipTrigger>
                  <TooltipContent
                    side="top"
                    sideOffset={6}
                    align="start"
                    className="max-w-64 text-left leading-relaxed whitespace-normal"
                  >
                    {t('devices.settings.sync.help')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={t('devices.settings.sync.restoreDefaults')}
              title={t('devices.settings.sync.restoreDefaults')}
              onClick={handleRestoreDefaults}
              disabled={globalAutoSyncOff || isLoading}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw className="size-3.5" />
            </Button>
          </div>

          {showSkeleton ? (
            <SyncSettingsSkeleton />
          ) : (
            <SyncSettingsTable
              sendEnabled={sendEnabled}
              receiveEnabled={receiveEnabled}
              sendContentTypes={preferences?.sendContentTypes}
              receiveContentTypes={preferences?.receiveContentTypes}
              globalAutoSyncOff={globalAutoSyncOff}
              globalFileSyncOff={globalFileSyncOff}
              isLoading={isLoading}
              onSendEnabledChange={handleSendEnabledToggle}
              onReceiveEnabledChange={handleReceiveEnabledToggle}
              onSendContentTypeChange={handleSendContentTypeToggle}
              onReceiveContentTypeChange={handleReceiveContentTypeToggle}
            />
          )}
        </section>
      </div>
    </div>
  )
}

export default PeerDetailPanel

function protectionBadgeClass(status: MemberProtectionStatus): string {
  switch (status) {
    case 'protected':
      return ''
    case 'awaiting_readmission':
    case 'requires_readmission':
      return 'border-warning/30 bg-warning/10 text-warning'
    case 'legacy_unprotected':
    case 'recovery_required':
      return 'border-destructive/30 bg-destructive/10 text-destructive'
  }
}

const SyncSettingsSkeleton: React.FC = () => (
  <div className="mt-3 space-y-2">
    <Skeleton className="h-10 w-full rounded-md" />
    <div className="space-y-1">
      {[0, 1, 2, 3, 4].map(i => (
        <Skeleton key={i} className="h-10 w-full rounded-md" />
      ))}
    </div>
  </div>
)
