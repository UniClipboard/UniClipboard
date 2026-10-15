export const INVITATION_CODE_LENGTH = 6

/** Default invitation lifetime, used to estimate progress when the issue time is unknown. */
export const INVITATION_DEFAULT_TTL_MS = 5 * 60 * 1000

/** Format a six-digit code as `XXX-XXX` without truncating unexpected values. */
export function formatInvitationCode(raw: string): string {
  const clean = raw.replace(/[\s-]/g, '')
  if (!/^\d{6}$/.test(clean)) return raw
  const midpoint = INVITATION_CODE_LENGTH / 2
  return `${clean.slice(0, midpoint)}-${clean.slice(midpoint)}`
}

/** Time left on an invitation as `mm:ss`. */
export function formatInvitationRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
}
