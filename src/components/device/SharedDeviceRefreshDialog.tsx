import { Loader2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type {
  SharedDeviceRefreshDeviceState,
  SharedDeviceRefreshSnapshot,
} from '@/api/daemon/member'
import StatusDot, { type StatusDotTone } from '@/components/device/StatusDot'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { SharedDeviceRefreshQueryFailure } from '@/hooks/useSharedDeviceRefresh'

interface SharedDeviceRefreshDialogProps {
  open: boolean
  snapshot: SharedDeviceRefreshSnapshot | null
  queryFailure: SharedDeviceRefreshQueryFailure | null
  onOpenChange: (open: boolean) => void
}

const SharedDeviceRefreshDialog: React.FC<SharedDeviceRefreshDialogProps> = ({
  open,
  snapshot,
  queryFailure,
  onOpenChange,
}) => {
  const { t } = useTranslation()

  const phaseLabel = snapshot
    ? snapshot.phase === 'round_completed'
      ? t('devices.sharedDeviceRefresh.roundCompleted')
      : snapshot.phase === 'connecting'
        ? t('devices.sharedDeviceRefresh.connecting')
        : t('devices.sharedDeviceRefresh.searching')
    : t('devices.sharedDeviceRefresh.searching')

  const showEmpty =
    snapshot?.phase === 'round_completed' && snapshot.devices.length === 0 && !queryFailure

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="min-h-64 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('devices.sharedDeviceRefresh.refresh')}</DialogTitle>
          <DialogDescription role="status" className="flex items-center gap-2">
            {!snapshot && !queryFailure && <Loader2 className="size-4 animate-spin" />}
            <span>
              {queryFailure === 'notFound' ? t('devices.sharedDeviceRefresh.ended') : phaseLabel}
            </span>
          </DialogDescription>
        </DialogHeader>

        <DialogBody className="flex flex-col gap-3">
          {queryFailure === 'temporary' && (
            <Alert className="border-warning/30 bg-warning/10 text-warning">
              <AlertDescription className="text-xs">
                {t('devices.sharedDeviceRefresh.updateFailed')}
              </AlertDescription>
            </Alert>
          )}

          {snapshot && snapshot.unavailableSourceCount > 0 && !queryFailure && (
            <Alert className="border-warning/30 bg-warning/10 text-warning">
              <AlertDescription className="text-xs">
                {t('devices.sharedDeviceRefresh.unavailableSources')}
              </AlertDescription>
            </Alert>
          )}

          {showEmpty ? (
            <p className="py-4 text-center text-sm text-muted-foreground">
              {t('devices.sharedDeviceRefresh.empty')}
            </p>
          ) : snapshot && snapshot.devices.length > 0 ? (
            <ul className="flex flex-col gap-2">
              {snapshot.devices.map(device => (
                <li
                  key={device.deviceId}
                  className="flex items-start justify-between gap-3 rounded-lg border border-border/50 bg-card px-3 py-2"
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <StatusDot tone={deviceStateTone(device.state)} />
                    <span className="min-w-0 truncate text-sm font-medium text-foreground">
                      {device.displayName || t('devices.sharedDeviceRefresh.unknownDevice')}
                    </span>
                  </span>
                  <span className="shrink-0 text-right text-xs text-muted-foreground">
                    {t(`devices.sharedDeviceRefresh.deviceStates.${device.state}`)}
                  </span>
                </li>
              ))}
            </ul>
          ) : !queryFailure ? (
            <div className="flex flex-1 items-center justify-center py-6">
              <Loader2 className="size-6 animate-spin text-muted-foreground" />
            </div>
          ) : null}
        </DialogBody>
      </DialogContent>
    </Dialog>
  )
}

function deviceStateTone(state: SharedDeviceRefreshDeviceState): StatusDotTone {
  switch (state) {
    case 'connected':
      return 'success'
    case 'connecting':
    case 'discovered':
      return 'info'
    case 'waiting_for_peer':
    case 'waiting_for_update':
    case 'version_incompatible':
    case 'rejected':
      return 'warning'
    case 'already_present':
      return 'off'
    default:
      return 'off'
  }
}

export default SharedDeviceRefreshDialog
