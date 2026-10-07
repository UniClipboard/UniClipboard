import { createHash } from 'node:crypto'
import { createServer } from 'node:net'

// Shared by the development launchers: profile names and the frontend dev-server port derived from them.
export const PROFILE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/
const PROFILE_PORT_BASE = 20_000
const PROFILE_PORT_RANGE = 20_000

export function devServerPortForProfile(profile) {
  const digest = createHash('sha256').update(profile).digest()
  return PROFILE_PORT_BASE + (digest.readUInt32BE(0) % PROFILE_PORT_RANGE)
}

function availablePort(port, host) {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen({ port, host, exclusive: true }, () => {
      const selectedPort = server.address().port
      server.close(error => (error ? reject(error) : resolve(selectedPort)))
    })
  })
}

export async function availableDevPort(port, host) {
  if (host) return availablePort(port, host)
  // macOS permits a wildcard listener alongside an existing loopback listener.
  // Probe each exact loopback address so another worktree cannot satisfy the dev server wait.
  const selectedPort = await availablePort(port, '127.0.0.1')
  try {
    await availablePort(selectedPort, '::1')
  } catch (error) {
    if (error.code !== 'EAFNOSUPPORT' && error.code !== 'EADDRNOTAVAIL') throw error
  }
  return selectedPort
}
