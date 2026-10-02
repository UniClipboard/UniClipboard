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
