import { ChevronRight, CircleHelp, Unlink } from 'lucide-react'
import React, { useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { ContentTypes, MemberProtectionStatus } from '@/api/daemon/member'
import { DEFAULT_SEND_CONTENT_TYPES } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import { deriveBadgeKind, derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import CopyIconButton from '@/components/device/CopyIconButton'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import { getDeviceIcon } from '@/components/device/device-utils'
import PanelFactRow from '@/components/device/PanelFactRow'
import StatusDot from '@/components/device/StatusDot'
import SyncPreferencesControl from '@/components/device/SyncPreferencesControl'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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
  status?: DeviceRowStatus
  device: SpaceMember | undefined
  globalSyncOff: boolean
  globalFileSyncOff: boolean
  /** Whether LAN-only mode is active (drives the derived channel label). */
  lanOnlyActive: boolean
  onUnpair: (peerId: string) => void
}

const PeerDetailPanel: React.FC<PeerDetailPanelProps> = ({
  deviceId,
  status,
  device,
  globalSyncOff,
  globalFileSyncOff,
  lanOnlyActive,
  onUnpair,
}) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  const preferences = useAppSelector(state => state.devices.memberSyncPreferences[deviceId])
  const sendEnabled = preferences?.sendEnabled ?? true
  const receiveEnabled = preferences?.receiveEnabled ?? true
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

  const handleSyncEnabledChange = useCallback(
    (checked: boolean) => {
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch: { sendEnabled: checked, receiveEnabled: checked },
        })
      )
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update device sync preference')
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure]
  )

  const handleContentDirectionChange = useCallback(
    (field: keyof ContentTypes, send: boolean, receive: boolean) => {
      const patch = {
        sendContentTypes: { [field]: send },
        receiveContentTypes: { [field]: receive },
        ...(!sendEnabled && send ? { sendEnabled: true } : {}),
        ...(!receiveEnabled && receive ? { receiveEnabled: true } : {}),
      }
      dispatch(
        updateMemberSyncPreferences({
          deviceId,
          patch,
        })
      )
        .unwrap()
        .catch(err => {
          handlePreferenceUpdateFailure(err, 'failed to update content sync direction', {
            field,
          })
        })
    },
    [dispatch, deviceId, handlePreferenceUpdateFailure, receiveEnabled, sendEnabled]
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
  const Icon = getDeviceIcon(device?.deviceName)

  return (
    <div className="@container min-h-full w-full bg-muted/20">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-7 px-5 py-8 @md:px-8 @lg:py-10">
        <header className="flex items-center gap-4">
          <div
            className={cn(
              'flex size-16 shrink-0 items-center justify-center rounded-xl border border-border/60 bg-card',
              connected ? 'text-foreground' : 'text-muted-foreground'
            )}
          >
            {/* eslint-disable-next-line react-hooks/static-components -- `getDeviceIcon` returns a stable lucide icon reference keyed on deviceName, not a freshly-created component */}
            <Icon className="size-8" strokeWidth={1.5} />
          </div>
          <div className="min-w-0 flex-1">
            <h3
              title={deviceName}
              className="truncate text-2xl font-semibold tracking-tight text-foreground"
            >
              {deviceName}
            </h3>
            <p className="mt-2 flex flex-wrap items-center gap-2 text-xs">
              <StatusDot tone={status ? 'warning' : dotTone} />
              <span
                className={cn(
                  'font-medium',
                  status
                    ? 'text-warning'
                    : connected && device?.channel === 'relay'
                      ? 'text-info'
                      : connected && device?.channel === 'direct'
                        ? 'text-success'
                        : 'text-muted-foreground'
                )}
              >
                {status?.label ??
                  (connected ? t('devices.list.status.online') : t('devices.list.status.offline'))}
              </span>
              <span className="text-muted-foreground/50">·</span>
              <span className="text-muted-foreground">{channelLabel}</span>
            </p>
          </div>
          <Button
            data-testid="device-unpair"
            variant="ghost"
            size="sm"
            aria-label={t('devices.list.actions.unpair')}
            title={t('devices.list.actions.unpair')}
            className="shrink-0 text-destructive hover:bg-destructive/10 hover:text-destructive"
            onClick={() => onUnpair(deviceId)}
          >
            <Unlink className="size-3.5" />
            <span>{t('devices.list.actions.unpair')}</span>
          </Button>
        </header>

        <div className="flex min-w-0 flex-col gap-5">
          <section className="min-w-0 overflow-hidden rounded-xl border border-border/60 bg-card text-card-foreground">
            <SyncPreferencesControl
              deviceName={deviceName}
              sendEnabled={sendEnabled}
              receiveEnabled={receiveEnabled}
              sendContentTypes={preferences?.sendContentTypes}
              receiveContentTypes={preferences?.receiveContentTypes}
              globalSyncOff={globalSyncOff}
              globalFileSyncOff={globalFileSyncOff}
              isLoading={!preferences}
              onSyncEnabledChange={handleSyncEnabledChange}
              onContentDirectionChange={handleContentDirectionChange}
              onRestoreDefaults={handleRestoreDefaults}
            />
          </section>
          <details
            open={!!protectionStatus && protectionStatus !== 'protected'}
            className="group overflow-hidden rounded-xl border border-border/60 bg-card text-card-foreground"
          >
            <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-5 py-4 text-sm font-medium transition-colors hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring @md:px-6 [&::-webkit-details-marker]:hidden">
              {t('devices.settings.sections.connection')}
              <ChevronRight className="size-4 text-muted-foreground transition-transform group-open:rotate-90" />
            </summary>
            <div className="flex flex-col border-t border-border/50 px-5 py-2 @md:px-6 [&>div]:py-3">
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
          </details>
        </div>
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
