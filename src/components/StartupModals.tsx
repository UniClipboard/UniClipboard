import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'
import RePairingNotice from './RePairingNotice'
import TelemetryNotice from './TelemetryNotice'

/**
 * Phases of the launch-time modal queue. Only one modal is rendered at a
 * time; phases advance when the active modal calls its `onDismiss`.
 *
 * Priority is fixed: Engine-owned re-pairing requirement first, telemetry
 * second. This component only mounts after the encryption gate has opened.
 */
type Phase = 'telemetry' | 'done'

/**
 * Single coordinator for startup-time modals. Mount once near the top of the
 * app shell; replace any standalone `<TelemetryNotice />` mounts.
 */
export default function StartupModals() {
  const navigate = useNavigate()
  const { rePairingRequired } = useSetupRealtimeStore()
  const [phase, setPhase] = useState<Phase>('telemetry')
  const [rePairingNoticeHandled, setRePairingNoticeHandled] = useState(false)

  const handleTelemetryDismissed = () => {
    setPhase('done')
  }

  const handleOpenDevices = () => {
    setRePairingNoticeHandled(true)
    setPhase('done')
    navigate('/devices')
  }

  if (rePairingRequired && !rePairingNoticeHandled) {
    return <RePairingNotice onOpenDevices={handleOpenDevices} />
  }

  return <TelemetryNotice enabled={phase === 'telemetry'} onDismiss={handleTelemetryDismissed} />
}
