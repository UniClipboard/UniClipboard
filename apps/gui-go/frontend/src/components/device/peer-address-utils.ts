import type { PeerAddressDto } from '@/api/generated/types.gen'

function isIPv4(value: string): boolean {
  const parts = value.split('.')
  return (
    parts.length === 4 &&
    parts.every(part => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255)
  )
}

function isIPv6(value: string): boolean {
  let hex = value
  if (hex.includes('.')) {
    const lastColon = hex.lastIndexOf(':')
    if (lastColon < 0 || !isIPv4(hex.slice(lastColon + 1))) return false
    hex = `${hex.slice(0, lastColon)}:0:0`
  }
  if (!/^[\da-f:]+$/i.test(hex)) return false
  const halves = hex.split('::')
  if (halves.length > 2) return false
  const groups = halves.flatMap(half => (half === '' ? [] : half.split(':')))
  if (!groups.every(group => /^[\da-f]{1,4}$/i.test(group))) return false
  return halves.length === 2 ? groups.length < 8 : groups.length === 8
}

/** Match Engine's literal-only grammar and reject unspecified and multicast addresses. */
export function validatePeerAddress(
  ipInput: string,
  portInput: string
): { valid: true; address: PeerAddressDto } | { valid: false; field: 'ip' | 'port' } {
  let ip = ipInput.trim()
  if (ip.startsWith('[') && ip.endsWith(']')) ip = ip.slice(1, -1)
  const v4 = isIPv4(ip)
  const v6 = !v4 && isIPv6(ip)
  if (
    (!v4 && !v6) ||
    (v4 &&
      (ip === '0.0.0.0' || (Number(ip.split('.')[0]) >= 224 && Number(ip.split('.')[0]) <= 239))) ||
    (v6 && (/^ff[\da-f]{2}:/i.test(ip) || new URL(`http://[${ip}]`).hostname === '[::]'))
  ) {
    return { valid: false, field: 'ip' }
  }
  // Brackets are accepted only around IPv6, matching Engine normalization.
  if (v4 && ipInput.trim().startsWith('[')) return { valid: false, field: 'ip' }
  const port = portInput.trim()
  if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
    return { valid: false, field: 'port' }
  }
  return { valid: true, address: { ip, port: Number(port) } }
}
