import { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { INVITATION_DEFAULT_TTL_MS, formatInvitationCode } from '@/lib/invitation-code'

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
}

/**
 * Countdown of an issued invitation. When the code expires the invitation is cancelled once per
 * code, so the sponsor returns to the space-ready step.
 */
export function useInvitationCountdown(code: string, expiresAtMs: number, onCancel: () => void) {
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [])

  const remaining = expiresAtMs - now
  const expired = remaining <= 0
  const progress = expired
    ? 0
    : Math.min(100, (remaining / Math.max(INVITATION_DEFAULT_TTL_MS, remaining)) * 100)
  const display = useMemo(() => formatInvitationCode(code), [code])
  const handledExpiryCodeRef = useRef<string | null>(null)
  const cancelExpiredInvitation = useEffectEvent(onCancel)

  // Key the guard on `code` so a fresh invitation re-arms auto-cancel even if
  // this screen stays mounted, while still firing at most once per code
  // (including under StrictMode's double-invoked effects).
  useEffect(() => {
    if (!expired || handledExpiryCodeRef.current === code) return
    handledExpiryCodeRef.current = code
    cancelExpiredInvitation()
  }, [expired, code])

  return { display, expired, progress, remainingLabel: formatRemaining(remaining) }
}
