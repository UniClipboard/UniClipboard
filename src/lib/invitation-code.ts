export const INVITATION_CODE_LENGTH = 6

/** Format a six-digit code as `XXX-XXX` without truncating unexpected values. */
export function formatInvitationCode(raw: string): string {
  const clean = raw.replace(/[\s-]/g, '')
  if (!/^\d{6}$/.test(clean)) return raw
  const midpoint = INVITATION_CODE_LENGTH / 2
  return `${clean.slice(0, midpoint)}-${clean.slice(midpoint)}`
}
