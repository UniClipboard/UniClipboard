import { Clock, Info } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { CurrentInvitation } from '@/api/daemon/setupV2'
import { LegacyCopyFullInvitationButton } from '@/components/LegacyCopyFullInvitationButton'
import { Progress } from '@/components/ui/progress'
import { formatInvitationRemaining } from '@/lib/invitation-code'
import { cn } from '@/lib/utils'

export function AddDeviceInvitation({
  invitation,
  expired,
  display,
  progress,
  remaining,
}: {
  invitation: CurrentInvitation
  expired: boolean
  display: string
  progress: number
  remaining: number
}) {
  const { t } = useTranslation()
  return (
    <div className="space-y-4 py-2">
      {/* 邀请码主卡 — 视觉焦点 */}
      <div
        className={cn(
          'rounded-2xl border bg-gradient-to-br p-5 transition-colors',
          expired
            ? 'border-destructive/30 from-destructive/5 to-transparent'
            : 'border-primary/20 from-primary/[0.04] to-transparent'
        )}
      >
        <div
          data-testid="add-device-invitation-code"
          className={cn(
            'select-all text-center font-mono font-semibold tabular-nums text-foreground',
            'text-ui-title',
            expired && 'text-muted-foreground/50 line-through decoration-1'
          )}
          aria-label={invitation.code}
        >
          {display}
        </div>

        <div className="mt-5 space-y-2">
          <Progress
            value={progress}
            className={cn('h-1', expired && '[&>[data-slot=progress-indicator]]:bg-destructive')}
          />
          <div
            className={cn(
              'flex items-center justify-center gap-1.5 text-ui-body tabular-nums',
              expired ? 'text-destructive' : 'text-muted-foreground'
            )}
          >
            <Clock className="size-3" />
            {expired
              ? t('devices.addDevice.expired')
              : t('devices.addDevice.expiresIn', {
                  remaining: formatInvitationRemaining(remaining),
                })}
          </div>
        </div>
      </div>

      {invitation.fullInvitation && (
        <LegacyCopyFullInvitationButton invitation={invitation.fullInvitation} disabled={expired} />
      )}
      {/* The joining device also needs the space passphrase. */}
      <div className="flex items-start gap-2.5 rounded-lg bg-muted/50 px-3.5 py-2.5 text-ui-body text-muted-foreground">
        <Info className="mt-0.5 size-3.5 shrink-0" />
        <span>{t('devices.addDevice.passphraseHint')}</span>
      </div>
    </div>
  )
}
