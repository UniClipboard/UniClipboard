export const INVITATION_CODE_LENGTH = 8

function cleanInvitationCode(raw: string): string {
  return raw.toUpperCase().replace(/[^A-Z0-9]/g, '')
}

/** Keep the eight alphanumeric characters accepted by invitation inputs. */
export function sanitizeInvitationCode(raw: string): string {
  return cleanInvitationCode(raw).slice(0, INVITATION_CODE_LENGTH)
}

/** Format a raw 8-char code as `XXXX-XXXX` for display surfaces. */
export function formatInvitationCode(raw: string): string {
  const clean = sanitizeInvitationCode(raw)
  if (clean.length <= 4) return clean
  return `${clean.slice(0, 4)}-${clean.slice(4, INVITATION_CODE_LENGTH)}`
}

export function isInvitationCodeComplete(raw: string): boolean {
  return cleanInvitationCode(raw).length === INVITATION_CODE_LENGTH
}

/** Normalize a complete invitation code for daemon API requests. */
export function normalizeInvitationCode(raw: string): string {
  if (!isInvitationCodeComplete(raw)) return raw
  return formatInvitationCode(raw)
}
