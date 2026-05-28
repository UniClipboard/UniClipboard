export const INVITATION_CODE_LENGTH = 8

/** Format a raw 8-char code as `XXXX-XXXX` for display surfaces. */
export function formatInvitationCode(raw: string): string {
  const clean = raw.toUpperCase().replace(/[^A-Z0-9]/g, '')
  if (clean.length <= 4) return clean
  return `${clean.slice(0, 4)}-${clean.slice(4, INVITATION_CODE_LENGTH)}`
}
