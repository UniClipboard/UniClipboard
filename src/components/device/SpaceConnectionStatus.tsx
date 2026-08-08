import { TriangleAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { MembershipConvergenceState } from '@/api/daemon/member'

interface SpaceConnectionStatusProps {
  state: MembershipConvergenceState
}

/**
 * Alerts only for convergence states the user can act on. Routine convergence
 * (`converging`) is invisible: it carries no actionable information and can
 * persist indefinitely while a peer stays offline. The shared-device refresh
 * window reports per-device progress instead.
 */
export default function SpaceConnectionStatus({ state }: SpaceConnectionStatusProps) {
  const { t } = useTranslation()

  if (state === 'complete' || state === 'converging') return null

  const label =
    state === 'waiting_for_upgrade'
      ? t('devices.convergence.waitingForUpgrade')
      : t('devices.convergence.blocked')

  return (
    <output className="mt-1.5 flex items-start gap-1.5 text-[11px] leading-4 text-warning">
      <TriangleAlert className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
      <span>{label}</span>
    </output>
  )
}
