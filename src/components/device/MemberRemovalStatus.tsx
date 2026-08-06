import { CircleCheck, ShieldAlert } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { MemberRemoval } from '@/api/daemon/member'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface MemberRemovalStatusProps {
  removal: MemberRemoval
  deviceNames: ReadonlyMap<string, string>
  onPermanentLoss: (removal: MemberRemoval, deviceIds: string[]) => void
  className?: string
}

const MemberRemovalStatus: React.FC<MemberRemovalStatusProps> = ({
  removal,
  deviceNames,
  onPermanentLoss,
  className,
}) => {
  const { t } = useTranslation()
  const isComplete = removal.outcome === 'complete'
  const requiresRecovery = removal.outcome === 'recovery_required'
  const pendingDeviceIds = removal.pendingRecipientDeviceIds
  const canMarkPermanentlyLost = removal.outcome === 'applied' && pendingDeviceIds.length > 0

  return (
    <section
      aria-live="polite"
      className={cn(
        '@container overflow-hidden rounded-lg border bg-card/95 text-card-foreground shadow-sm backdrop-blur-md',
        isComplete
          ? 'border-success/25'
          : requiresRecovery
            ? 'border-destructive/25'
            : 'border-warning/25',
        className
      )}
    >
      <div className="flex items-start gap-2.5 px-3 pt-3 pb-2.5">
        <div
          className={cn(
            'mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-md border',
            isComplete
              ? 'border-success/20 bg-success/10 text-success'
              : requiresRecovery
                ? 'border-destructive/20 bg-destructive/10 text-destructive'
                : 'border-warning/20 bg-warning/10 text-warning'
          )}
        >
          {isComplete ? (
            <CircleCheck className="size-4" aria-hidden="true" />
          ) : (
            <ShieldAlert className="size-4" aria-hidden="true" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-xs font-semibold text-foreground">
            {isComplete
              ? t('devices.memberRemoval.complete.title')
              : requiresRecovery
                ? t('devices.memberRemoval.recovery.title')
                : t('devices.memberRemoval.pending.title')}
          </h3>
          <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
            {isComplete
              ? t('devices.memberRemoval.complete.description')
              : requiresRecovery
                ? t('devices.memberRemoval.recovery.description')
                : t('devices.memberRemoval.pending.description', {
                    count: removal.pendingRecipients,
                  })}
          </p>
        </div>
      </div>
      {!isComplete && pendingDeviceIds.length > 0 && (
        <div className="border-t border-border/50 bg-muted/[0.16] px-3 py-2.5">
          <ul
            className="flex min-w-0 flex-wrap gap-1.5"
            aria-label={t('devices.memberRemoval.pending.title')}
          >
            {pendingDeviceIds.map(deviceId => (
              <li
                key={deviceId}
                className="max-w-full truncate rounded-md border border-border/50 bg-card/60 px-2 py-0.5 text-[11px] font-medium text-muted-foreground"
                title={deviceNames.get(deviceId) ?? deviceId}
              >
                {deviceNames.get(deviceId) ?? deviceId}
              </li>
            ))}
          </ul>
          {canMarkPermanentlyLost && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-2 h-auto min-h-7 w-full border-current bg-card/50 px-2 py-1 text-[11px] font-medium leading-4 whitespace-normal text-warning hover:bg-current/10 hover:text-current"
              onClick={() => onPermanentLoss(removal, pendingDeviceIds)}
            >
              {t('devices.memberRemoval.permanentLoss.action')}
            </Button>
          )}
        </div>
      )}
    </section>
  )
}

export default MemberRemovalStatus
