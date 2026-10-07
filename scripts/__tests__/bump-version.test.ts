import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { bumpVersion, parseSemver } from '../bump-version-lib.js'
import { updateCargoToml } from '../bump-version.js'

describe('workspace version inheritance', () => {
  it('uc-dev-cli uses version.workspace = true', () => {
    const content = fs.readFileSync(
      path.resolve(__dirname, '../../tools/uc-dev-cli/Cargo.toml'),
      'utf8'
    )
    expect(content).toContain('version.workspace = true')
    expect(content).not.toMatch(/^version\s*=\s*"[\d.]+"/m)
  })
})

describe('bumpVersion prerelease patch', () => {
  it('keeps base version when creating first alpha prerelease', () => {
    expect(bumpVersion('0.1.0', 'patch', 'alpha')).toBe('0.1.0-alpha.1')
  })

  it('increments alpha prerelease number on repeat', () => {
    expect(bumpVersion('0.1.0-alpha.1', 'patch', 'alpha')).toBe('0.1.0-alpha.2')
  })
})

describe('parseSemver target version', () => {
  it('accepts explicit prerelease target version', () => {
    const parsed = parseSemver('0.1.0-alpha.2')
    expect(parsed.prerelease).toBe('alpha')
    expect(parsed.prereleaseVersion).toBe(2)
  })
})

describe('updateCargoToml', () => {
  const tempDirs: string[] = []

  afterEach(() => {
    while (tempDirs.length > 0) {
      const dir = tempDirs.pop()
      if (dir) {
        fs.rmSync(dir, { recursive: true, force: true })
      }
    }
  })

  it('updates only the package version and preserves dependency versions', () => {
    const repoDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-version-'))
    tempDirs.push(repoDir)
    const manifestPath = path.join(repoDir, 'Cargo.toml')
    fs.writeFileSync(
      manifestPath,
      '[package]\nname = "example"\nversion = "0.20.0-alpha.5"\n\n[dependencies.helper-a]\nversion = "1"\noptional = true\n\n[dependencies.helper-b]\nversion = "1"\noptional = true\n',
      'utf8'
    )

    updateCargoToml('0.20.0-alpha.6', false, path.relative(process.cwd(), manifestPath), 'package')

    expect(fs.readFileSync(manifestPath, 'utf8')).toBe(
      '[package]\nname = "example"\nversion = "0.20.0-alpha.6"\n\n[dependencies.helper-a]\nversion = "1"\noptional = true\n\n[dependencies.helper-b]\nversion = "1"\noptional = true\n'
    )
  })

  it('updates only the workspace package version', () => {
    const repoDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-version-'))
    tempDirs.push(repoDir)
    const manifestPath = path.join(repoDir, 'Cargo.toml')
    fs.writeFileSync(
      manifestPath,
      '[workspace.package]\nversion = "0.20.0-alpha.5"\n\n[dependencies.release-helper]\nversion = "2"\n',
      'utf8'
    )

    updateCargoToml(
      '0.20.0-alpha.6',
      false,
      path.relative(process.cwd(), manifestPath),
      'workspace.package'
    )

    expect(fs.readFileSync(manifestPath, 'utf8')).toBe(
      '[workspace.package]\nversion = "0.20.0-alpha.6"\n\n[dependencies.release-helper]\nversion = "2"\n'
    )
  })
})
