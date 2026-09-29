import { describe, expect, it } from 'vitest'
import { resolveAppEnv } from '../ci/resolve-app-env.mjs'

describe('APP_ENV resolution', () => {
  it.each([
    ['stable', 'production'],
    ['', 'production'],
    ['alpha', 'alpha'],
    ['beta', 'beta'],
    ['rc', 'rc'],
  ])('maps channel %j to %s', (channel, appEnv) => {
    expect(resolveAppEnv(channel)).toEqual({ appEnv, known: true })
  })

  it('falls back to production for unknown channels and reports it', () => {
    expect(resolveAppEnv('nightly')).toEqual({ appEnv: 'production', known: false })
  })
})
