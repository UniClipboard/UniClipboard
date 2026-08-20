import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { buildRegistration } from '../build-flare-release-registration.js'

describe('buildFlareReleaseRegistration', () => {
  const tempDirs: string[] = []

  afterEach(() => {
    for (const directory of tempDirs.splice(0))
      fs.rmSync(directory, { recursive: true, force: true })
  })

  it('maps the selected updater artifacts without promoting a channel', () => {
    const artifactsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'flare-registration-'))
    tempDirs.push(artifactsDir)
    fs.writeFileSync(path.join(artifactsDir, 'UniClipboard.app.tar.gz'), 'artifact')

    const registration = buildRegistration({
      version: '1.2.3-alpha.1',
      channel: 'alpha',
      manifest: {
        pub_date: '2026-08-20T12:00:00Z',
        notes: 'English notes\n\n<!-- zh -->\n\n中文说明',
        platforms: {
          'darwin-aarch64': {
            signature: 'signed',
            url: 'https://release.uniclipboard.app/artifacts/v1.2.3-alpha.1/UniClipboard.app.tar.gz',
          },
        },
      },
      artifactsDir,
      source: 'github-actions:run-123',
    })

    expect(registration).toMatchObject({
      product: 'desktop',
      version: '1.2.3-alpha.1',
      tagName: 'v1.2.3-alpha.1',
      prerelease: true,
      publishedAt: '2026-08-20T12:00:00Z',
      source: 'github-actions:run-123',
      notes: { en: 'English notes', zh: '中文说明' },
      artifacts: [
        {
          platform: 'darwin-aarch64',
          filename: 'UniClipboard.app.tar.gz',
          r2Key: 'artifacts/v1.2.3-alpha.1/UniClipboard.app.tar.gz',
          size: 8,
          signature: 'signed',
        },
      ],
    })
    expect(registration).not.toHaveProperty('channel')
  })

  it('rejects a manifest whose selected artifact is missing', () => {
    const artifactsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'flare-registration-'))
    tempDirs.push(artifactsDir)

    expect(() =>
      buildRegistration({
        version: '1.2.3',
        channel: 'stable',
        manifest: {
          pub_date: '2026-08-20T12:00:00Z',
          notes: 'English notes\n\n<!-- zh -->\n\nChinese notes',
          platforms: {
            'windows-x86_64': {
              signature: 'signed',
              url: 'https://release.uniclipboard.app/artifacts/v1.2.3/missing.exe',
            },
          },
        },
        artifactsDir,
        source: 'test',
      })
    ).toThrow('does not exist')
  })
})
