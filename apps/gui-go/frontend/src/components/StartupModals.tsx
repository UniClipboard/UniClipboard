import { useState } from 'react'
import { useNavigate } from 'react-router'
import RePairingNotice from '@/components/RePairingNotice'
import TelemetryNotice from '@/components/TelemetryNotice'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'

const RE_PAIRING_NOTICE_DISMISSED_KEY = 'uc-re-pairing-notice-dismissed'

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
  const [rePairingNoticeHandled, setRePairingNoticeHandled] = useState(
    () => localStorage.getItem(RE_PAIRING_NOTICE_DISMISSED_KEY) === '1'
  )

  const handleTelemetryDismissed = () => {
    setPhase('done')
  }

  const handleOpenDevices = () => {
    setRePairingNoticeHandled(true)
    setPhase('done')
    navigate('/devices')
  }

  const handleDontShowAgain = () => {
    localStorage.setItem(RE_PAIRING_NOTICE_DISMISSED_KEY, '1')
    setRePairingNoticeHandled(true)
  }

  if (rePairingRequired && !rePairingNoticeHandled) {
    return (
      <RePairingNotice onOpenDevices={handleOpenDevices} onDontShowAgain={handleDontShowAgain} />
    )
  }

  return <TelemetryNotice enabled={phase === 'telemetry'} onDismiss={handleTelemetryDismissed} />
}
