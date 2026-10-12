import { useState } from 'react'
import { useSettingSelector } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'

const log = createLogger('telemetry-notice')

const TELEMETRY_NOTICE_KEY = 'uc-telemetry-notice-seen'

export interface TelemetryNoticeProps {
  /** Coordinator-controlled gate. When false, the dialog stays hidden even if
   *  localStorage has no record — this lets a higher-priority startup modal
   *  display first. */
  enabled?: boolean
  /** Invoked once the user has chosen accept or opt-out. Used by
   *  `StartupModals` to advance the queue. */
  onDismiss?: () => void
}

/**
 * The first-run telemetry notice's consent logic, shared by every presentation of it
 * (`TelemetryNotice`, `TelemetryKitNotice`). Accepting keeps the stored settings as they are; only
 * opting out writes them.
 */
export function useTelemetryNotice({ enabled = true, onDismiss }: TelemetryNoticeProps) {
  const updateGeneralSetting = useSettingSelector(context => context.updateGeneralSetting)
  const [dismissed, setDismissed] = useState(() => !!localStorage.getItem(TELEMETRY_NOTICE_KEY))

  const finish = () => {
    localStorage.setItem(TELEMETRY_NOTICE_KEY, '1')
    setDismissed(true)
    onDismiss?.()
  }

  const accept = () => {
    finish()
  }

  const optOut = async () => {
    try {
      await updateGeneralSetting({ telemetryEnabled: false, usageAnalyticsEnabled: false })
      finish()
    } catch (error) {
      log.error({ err: error }, 'Failed to disable telemetry')
      // Don't close — let the user retry or accept instead.
    }
  }

  return { open: enabled && !dismissed, accept, optOut }
}
