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

/** Classify locally without decoding admission material or changing its case. */
export function classifyInvitationInput(raw: string): 'short_code' | 'full_invitation' | 'invalid' {
  const trimmed = raw.trim()
  if (trimmed.startsWith('ucspace1_') && trimmed.length > 'ucspace1_'.length) {
    return 'full_invitation'
  }
  return /^\d{6}$/.test(trimmed.replace(/[\s-]/g, '')) ? 'short_code' : 'invalid'
}

/** Keep the numeric control empty while a full invitation is being entered. */
export function shortInvitationInput(raw: string): string {
  return /^[\d\s-]*$/.test(raw) ? raw : ''
}

/** Full invitations are opaque and case-sensitive; only surrounding whitespace is removed. */
export function normalizeInvitationInput(raw: string): string {
  const trimmed = raw.trim()
  return trimmed.startsWith('ucspace1_') ? trimmed : formatInvitationCode(raw)
}

/** Time left on an invitation as `mm:ss`. */
export function formatInvitationRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
}
