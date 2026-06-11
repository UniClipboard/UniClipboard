import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { buildInstallerTable, readChangelogSection } from '../generate-release-notes.js'

const BASE_URL = 'https://example.com/dl'

let artifactsDir: string

function seed(fileNames: string[]) {
  for (const name of fileNames) {
    fs.writeFileSync(path.join(artifactsDir, name), '')
  }
}

beforeEach(() => {
  artifactsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-release-notes-'))
})

afterEach(() => {
  fs.rmSync(artifactsDir, { recursive: true, force: true })
})

describe('buildInstallerTable Windows rows', () => {
  it('lists x64/arm64 setup and portable separately with correct arch labels', () => {
    // Regression: a release shipping both x64 and arm64 installers. Files sort
    // alphabetically, so arm64-setup.exe precedes x64-setup.exe — the old code
    // picked the first .exe and hard-labeled it x86_64, mislabeling arm64.
    seed([
      'UniClipboard_0.13.0-alpha.4_arm64-portable.zip',
      'UniClipboard_0.13.0-alpha.4_arm64-setup.exe',
      'UniClipboard_0.13.0-alpha.4_x64-portable.zip',
      'UniClipboard_0.13.0-alpha.4_x64-setup.exe',
    ])

    const table = buildInstallerTable({ artifactsDir, baseUrl: BASE_URL })

    expect(table).toContain(
      '| Windows | x86_64 (Installer) | [UniClipboard_0.13.0-alpha.4_x64-setup.exe]'
    )
    expect(table).toContain(
      '| Windows | ARM64 (Installer) | [UniClipboard_0.13.0-alpha.4_arm64-setup.exe]'
    )
    expect(table).toContain(
      '| Windows | x86_64 (Portable) | [UniClipboard_0.13.0-alpha.4_x64-portable.zip]'
    )
    expect(table).toContain(
      '| Windows | ARM64 (Portable) | [UniClipboard_0.13.0-alpha.4_arm64-portable.zip]'
    )
    // The arm64 installer must never be advertised under an x86_64 label.
    expect(table).not.toContain(
      '| Windows | x86_64 (Installer) | [UniClipboard_0.13.0-alpha.4_arm64-setup.exe]'
    )
  })

  it('emits a single x86_64 installer row for legacy x64-only releases', () => {
    seed(['UniClipboard_0.13.0-alpha.3_x64-setup.exe'])

    const table = buildInstallerTable({ artifactsDir, baseUrl: BASE_URL })

    expect(table).toContain(
      '| Windows | x86_64 (Installer) | [UniClipboard_0.13.0-alpha.3_x64-setup.exe]'
    )
    expect(table).not.toContain('ARM64')
    expect(table).not.toContain('Portable')
  })
})

describe('readChangelogSection pinned announcement', () => {
  it('prepends announcement.md from the changelog directory', () => {
    const changelogPath = path.join(artifactsDir, '0.14.1.md')
    fs.writeFileSync(changelogPath, '## 0.14.1\n\n- A fix\n', 'utf8')
    fs.writeFileSync(
      path.join(artifactsDir, 'announcement.md'),
      '> **Notice**: reinstall.\n',
      'utf8'
    )

    expect(readChangelogSection(changelogPath)).toBe(
      '> **Notice**: reinstall.\n\n## 0.14.1\n\n- A fix'
    )
  })

  it('prepends the announcement even when the changelog file is missing', () => {
    fs.writeFileSync(
      path.join(artifactsDir, 'announcement.md'),
      '> **Notice**: reinstall.\n',
      'utf8'
    )

    expect(readChangelogSection(path.join(artifactsDir, '0.14.1.md'))).toBe(
      "> **Notice**: reinstall.\n\n## What's Changed\n\nRelease notes are not available yet."
    )
  })

  it('returns the changelog unchanged when no announcement exists', () => {
    const changelogPath = path.join(artifactsDir, '0.14.1.md')
    fs.writeFileSync(changelogPath, '## 0.14.1\n\n- A fix\n', 'utf8')

    expect(readChangelogSection(changelogPath)).toBe('## 0.14.1\n\n- A fix')
  })
})
