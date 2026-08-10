import assert from 'node:assert/strict'
import test from 'node:test'

import config from '../next.config.mjs'

test('serves the independent documentation site from the domain root', () => {
  assert.equal(config.basePath, undefined)
  assert.equal(config.assetPrefix, undefined)
  assert.equal(config.supportsImmutableAssets, undefined)
})
