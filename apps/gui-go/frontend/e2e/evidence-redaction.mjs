// Redact transport credentials and pairing codes in diagnostics, never in assertion inputs.
export function redactEvidence(text) {
  return text
    .replace(/eyJ[\w-]+\.[\w-]+\.[\w-]+/g, '[redacted session]')
    .replace(/(Bearer|Session)(?:%20|\s+)[A-Za-z0-9._~-]+/gi, '$1 [redacted]')
    .replace(/\b\d{3}-?\d{3}\b/g, '[redacted invitation code]')
}
