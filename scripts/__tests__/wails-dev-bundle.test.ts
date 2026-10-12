import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { makeDevBundle } from '../wails-dev.mjs'

// The Go host only drives the native quick panel (global shortcut, modifier double tap) when the
// helper executable sits next to its own executable; without it the macOS WebView panel is used,
// which has no modifier double-tap trigger.
describe('macOS development bundle', () => {
  it.skipIf(process.platform !== 'darwin')(
    'ships the native quick panel helper next to the host executable',
    () => {
      const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-dev-bundle-'))
      try {
        const host = path.join(sandbox, 'gui-go-dev')
        const helper = path.join(sandbox, 'uniclip-quick-panel')
        fs.copyFileSync('/usr/bin/true', host)
        fs.copyFileSync('/usr/bin/true', helper)

        const executable = makeDevBundle(host, helper)

        const bundled = path.join(path.dirname(executable), 'uniclip-quick-panel')
        expect(fs.statSync(bundled).mode & 0o111).not.toBe(0)
        const verify = spawnSync('codesign', [
          '--verify',
          '--strict',
          path.join(sandbox, 'UniClipboardGoDev.app'),
        ])
        expect(verify.status).toBe(0)
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    },
    30_000
  )

  it.skipIf(process.platform !== 'darwin')(
    'leaves the helper out, and removes an old one, when none is given',
    () => {
      const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-dev-bundle-'))
      try {
        const host = path.join(sandbox, 'gui-go-dev')
        const helper = path.join(sandbox, 'uniclip-quick-panel')
        fs.copyFileSync('/usr/bin/true', host)
        fs.copyFileSync('/usr/bin/true', helper)
        const executable = makeDevBundle(host, helper)

        makeDevBundle(host, null)

        expect(fs.existsSync(path.join(path.dirname(executable), 'uniclip-quick-panel'))).toBe(
          false
        )
      } finally {
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    },
    30_000
  )
})
