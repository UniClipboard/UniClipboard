import { LoaderCircle, TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { MembershipConvergenceState } from '@/api/daemon/member'
import { cn } from '@/lib/utils'

interface SpaceConnectionStatusProps {
  state: MembershipConvergenceState
}

export default function SpaceConnectionStatus({ state }: SpaceConnectionStatusProps) {
  const { t } = useTranslation()

  if (state === 'complete') return null

  const warning = state !== 'converging'
  const label =
    state === 'converging'
      ? t('devices.convergence.converging')
      : state === 'waiting_for_upgrade'
        ? t('devices.convergence.waitingForUpgrade')
        : t('devices.convergence.blocked')

  return (
    <div
      className={cn(
        'mt-1.5 flex items-start gap-1.5 text-[11px] leading-4',
        warning ? 'text-warning' : 'text-muted-foreground'
      )}
      role="status"
    >
      {warning ? (
        <TriangleAlert className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
      ) : (
        <LoaderCircle className="mt-0.5 size-3 shrink-0 animate-spin" aria-hidden="true" />
      )}
      <span>{label}</span>
    </div>
  )
}
