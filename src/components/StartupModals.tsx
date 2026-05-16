import { useEffect, useState } from 'react'
import { getUpgradeStatus, type UpgradeStatus } from '@/api/daemon'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { createLogger } from '@/lib/logger'
import TelemetryNotice from './TelemetryNotice'
import UpgradeNotice from './UpgradeNotice'

const log = createLogger('startup-modals')

/**
 * Phases of the launch-time modal queue. Only one modal is rendered at a
 * time; phases advance when the active modal calls its `onDismiss`.
 *
 * Priority is fixed: upgrade first (action-required for old users to keep
 * sync working), telemetry second (privacy consent for fresh installs).
 * In practice these two are mutually exclusive — `Upgraded { from: null }`
 * implies the user already had the app installed and almost certainly
 * already dismissed the telemetry notice. The explicit ordering only
 * matters in the edge case where browser localStorage was wiped.
 */
type Phase = 'loading' | 'upgrade' | 'telemetry' | 'done'

/**
 * Single coordinator for startup-time modals. Mount once near the top of the
 * app shell; replace any standalone `<TelemetryNotice />` mounts.
 */
export default function StartupModals() {
  const [phase, setPhase] = useState<Phase>('loading')
  const [upgradeStatus, setUpgradeStatus] = useState<UpgradeStatus | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        await connectDaemonWs()
        const status = await getUpgradeStatus()
        if (cancelled) return
        setUpgradeStatus(status)
        if (status.kind === 'upgraded' && status.from === null) {
          setPhase('upgrade')
        } else {
          setPhase('telemetry')
        }
      } catch (err) {
        // Daemon unreachable or endpoint absent — fall through to telemetry
        // so a fresh-install consent prompt still works without the daemon.
        log.warn({ err }, 'failed to fetch upgrade status; skipping upgrade modal')
        if (!cancelled) setPhase('telemetry')
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const handleUpgradeDismissed = () => {
    setPhase('telemetry')
  }

  const handleTelemetryDismissed = () => {
    setPhase('done')
  }

  return (
    <>
      {phase === 'upgrade' && (
        <UpgradeNotice status={upgradeStatus} onDismiss={handleUpgradeDismissed} />
      )}
      <TelemetryNotice enabled={phase === 'telemetry'} onDismiss={handleTelemetryDismissed} />
    </>
  )
}
