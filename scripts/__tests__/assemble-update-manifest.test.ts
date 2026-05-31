import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

function writeSigFile(filePath: string, content = 'mock-signature') {
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, content, 'utf8')
}

describe('assemble-update-manifest', () => {
  const tempDirs: string[] = []

  afterEach(() => {
    while (tempDirs.length > 0) {
      const dir = tempDirs.pop()
      if (dir) {
        fs.rmSync(dir, { recursive: true, force: true })
      }
    }
  })

  it('recognizes current release asset naming patterns', () => {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'assemble-manifest-'))
    tempDirs.push(tempDir)

    const artifactsDir = path.join(tempDir, 'release-assets')
    const outputPath = path.join(tempDir, 'updates', 'alpha.json')
    const baseUrl = 'https://github.com/UniClipboard/UniClipboard/releases/download/v0.1.0-alpha.2'

    writeSigFile(
      path.join(artifactsDir, 'UniClipboard_aarch64-apple-darwin.app.tar.gz.sig'),
      'sig-macos-arm64'
    )
    writeSigFile(
      path.join(artifactsDir, 'UniClipboard_x86_64-apple-darwin.app.tar.gz.sig'),
      'sig-macos-x64'
    )
    writeSigFile(
      path.join(artifactsDir, 'UniClipboard_0.1.0-alpha.2_amd64.AppImage.sig'),
      'sig-linux'
    )
    writeSigFile(path.join(artifactsDir, 'UniClipboard_0.1.0-alpha.2_x64-setup.exe.sig'), 'sig-win')
    writeSigFile(
      path.join(artifactsDir, 'UniClipboard_0.1.0-alpha.2_arm64-setup.exe.sig'),
      'sig-win-arm64'
    )
    writeSigFile(path.join(artifactsDir, 'UniClipboard-0.1.0-alpha.2-1.x86_64.rpm.sig'), 'sig-rpm')
    writeSigFile(path.join(artifactsDir, 'UniClipboard_0.1.0-alpha.2_amd64.deb.sig'), 'sig-deb')

    execFileSync(
      'node',
      [
        'scripts/assemble-update-manifest.js',
        '--version',
        '0.1.0-alpha.2',
        '--artifacts-dir',
        artifactsDir,
        '--output',
        outputPath,
        '--base-url',
        baseUrl,
      ],
      {
        cwd: process.cwd(),
        stdio: 'pipe',
      }
    )

    const manifest = JSON.parse(fs.readFileSync(outputPath, 'utf8')) as {
      version: string
      platforms: Record<string, { url: string; signature: string }>
    }

    expect(manifest.version).toBe('0.1.0-alpha.2')
    expect(Object.keys(manifest.platforms).sort()).toEqual([
      'darwin-aarch64',
      'darwin-x86_64',
      'linux-x86_64',
      'windows-aarch64',
      'windows-x86_64',
    ])

    expect(manifest.platforms['darwin-aarch64'].url).toBe(
      `${baseUrl}/UniClipboard_aarch64-apple-darwin.app.tar.gz`
    )
    expect(manifest.platforms['darwin-x86_64'].url).toBe(
      `${baseUrl}/UniClipboard_x86_64-apple-darwin.app.tar.gz`
    )
    expect(manifest.platforms['linux-x86_64'].url).toBe(
      `${baseUrl}/UniClipboard_0.1.0-alpha.2_amd64.AppImage`
    )
    expect(manifest.platforms['windows-x86_64'].url).toBe(
      `${baseUrl}/UniClipboard_0.1.0-alpha.2_x64-setup.exe`
    )
    expect(manifest.platforms['windows-aarch64'].url).toBe(
      `${baseUrl}/UniClipboard_0.1.0-alpha.2_arm64-setup.exe`
    )
  })
})
