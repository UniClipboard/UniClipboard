import assert from 'node:assert/strict'
import test from 'node:test'

import config from '../next.config.mjs'

test('disables immutable static assets for the path-mounted deployment', () => {
  assert.equal(config.supportsImmutableAssets, false)
})
