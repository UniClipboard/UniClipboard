import { createServer } from 'node:net'
import { describe, expect, it } from 'vitest'
import { availableDevPort, devServerPortForProfile, PROFILE_PATTERN } from '../dev-profile.mjs'

describe('development profile helpers', () => {
  it.each(['alice', 'a', 'Alice_1-b', 'a'.repeat(64)])('accepts profile name %j', profile => {
    expect(PROFILE_PATTERN.test(profile)).toBe(true)
  })

  it.each([' ', '../alice', 'alice/bob', 'alice\\bob', '-alice', 'alice.profile', 'a'.repeat(65)])(
    'rejects unsafe profile name %j',
    profile => {
      expect(PROFILE_PATTERN.test(profile)).toBe(false)
    }
  )

  it('assigns each profile its own stable frontend port', () => {
    expect(devServerPortForProfile('alice')).toBe(devServerPortForProfile('alice'))
    expect(devServerPortForProfile('alice')).not.toBe(devServerPortForProfile('bob'))
  })

  it.each(['127.0.0.1', '::1'])(
    'avoids an existing frontend listening on %s without stopping it',
    async host => {
      const preferred = devServerPortForProfile('occupied-profile')
      const server = createServer()
      await new Promise<void>((resolve, reject) => {
        server.once('error', reject)
        server.listen(preferred, host, resolve)
      })
      try {
        await expect(availableDevPort(preferred)).rejects.toMatchObject({ code: 'EADDRINUSE' })
        const port = await availableDevPort(0)
        expect(port).toBeGreaterThan(0)
        expect(port).not.toBe(preferred)
        expect(server.listening).toBe(true)
      } finally {
        await new Promise<void>(resolve => server.close(() => resolve()))
      }
    }
  )
})
