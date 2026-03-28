import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { generateReleaseNotes } from '../generate-release-notes.js'

describe('generate-release-notes', () => {
  const tempDirs: string[] = []
  const originalCwd = process.cwd()

  afterEach(() => {
    process.chdir(originalCwd)
    while (tempDirs.length > 0) {
      const dir = tempDirs.pop()
      if (dir) {
        fs.rmSync(dir, { recursive: true, force: true })
      }
    }
  })

  function setupRepo() {
    const repoDir = fs.mkdtempSync(path.join(os.tmpdir(), 'release-notes-'))
    tempDirs.push(repoDir)
    process.chdir(repoDir)

    fs.mkdirSync(path.join(repoDir, 'docs', 'changelog'), { recursive: true })
    fs.mkdirSync(path.join(repoDir, '.github', 'release-notes'), { recursive: true })
    fs.mkdirSync(path.join(repoDir, 'release-assets'), { recursive: true })

    fs.writeFileSync(
      path.join(repoDir, '.github', 'release-notes', 'release.md.tmpl'),
      '{{CHANGELOG_SECTION}}\n\n{{CHANGELOG_LINKS_SECTION}}\n\n## App Installation\n\n### macOS\n{{MACOS_INSTALLERS}}\n\n### Linux\n{{LINUX_INSTALLERS}}\n\n### Windows\n{{WINDOWS_INSTALLERS}}\n\n## CLI Downloads\n\n### macOS\n{{CLI_MACOS_INSTALLERS}}\n\n### Linux\n{{CLI_LINUX_INSTALLERS}}\n\n### Windows\n{{CLI_WINDOWS_INSTALLERS}}\n\n{{IS_PRERELEASE_WARNING}}\n',
      'utf8'
    )

    return repoDir
  }

  it('embeds english changelog and links chinese changelog when both exist', () => {
    const repoDir = setupRepo()
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '1.2.3.md'),
      "## What's Changed\n\n- English",
      'utf8'
    )
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '1.2.3.zh.md'),
      '## 更新\n\n- 中文',
      'utf8'
    )
    fs.writeFileSync(path.join(repoDir, 'release-assets', 'UniClipboard_aarch64.dmg'), 'x', 'utf8')
    fs.writeFileSync(
      path.join(repoDir, 'release-assets', 'UniClipboard_x64-setup.exe'),
      'x',
      'utf8'
    )

    const outputPath = path.join(repoDir, 'release-notes.md')
    generateReleaseNotes({
      version: '1.2.3',
      repo: 'foo/bar',
      previousTag: 'v1.2.2',
      channel: 'alpha',
      isPrerelease: true,
      artifactsDir: path.join(repoDir, 'release-assets'),
      template: path.join(repoDir, '.github', 'release-notes', 'release.md.tmpl'),
      output: outputPath,
    })

    const content = fs.readFileSync(outputPath, 'utf8')
    expect(content).toContain("## What's Changed")
    expect(content).toContain('- English')
    expect(content).toContain('中文变更日志')
    expect(content).toContain('1.2.3.zh.md')
    expect(content).toContain('Prerelease Warning')
    expect(content).toContain('Apple Silicon')
    expect(content).toContain('NSIS Installer')
  })

  it('includes CLI download links when CLI artifacts are present', () => {
    const repoDir = setupRepo()
    // Must update template to include CLI placeholders
    fs.writeFileSync(
      path.join(repoDir, '.github', 'release-notes', 'release.md.tmpl'),
      '{{CHANGELOG_SECTION}}\n\n## App Installation\n\n### macOS\n{{MACOS_INSTALLERS}}\n\n### Linux\n{{LINUX_INSTALLERS}}\n\n### Windows\n{{WINDOWS_INSTALLERS}}\n\n## CLI Downloads\n\n### macOS\n{{CLI_MACOS_INSTALLERS}}\n\n### Linux\n{{CLI_LINUX_INSTALLERS}}\n\n### Windows\n{{CLI_WINDOWS_INSTALLERS}}\n\n{{IS_PRERELEASE_WARNING}}\n',
      'utf8'
    )
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '1.2.3.md'),
      "## What's Changed\n\n- Feature",
      'utf8'
    )
    // App artifacts
    fs.writeFileSync(path.join(repoDir, 'release-assets', 'UniClipboard_aarch64.dmg'), 'x', 'utf8')
    // CLI artifacts
    fs.writeFileSync(
      path.join(repoDir, 'release-assets', 'uniclipboard-cli-1.2.3-aarch64-apple-darwin.tar.gz'),
      'x',
      'utf8'
    )
    fs.writeFileSync(
      path.join(
        repoDir,
        'release-assets',
        'uniclipboard-cli-1.2.3-x86_64-unknown-linux-gnu.tar.gz'
      ),
      'x',
      'utf8'
    )
    fs.writeFileSync(
      path.join(repoDir, 'release-assets', 'uniclipboard-cli-1.2.3-x86_64-pc-windows-msvc.zip'),
      'x',
      'utf8'
    )

    const outputPath = path.join(repoDir, 'release-notes.md')
    generateReleaseNotes({
      version: '1.2.3',
      repo: 'foo/bar',
      previousTag: 'v1.2.2',
      channel: 'stable',
      isPrerelease: false,
      artifactsDir: path.join(repoDir, 'release-assets'),
      template: path.join(repoDir, '.github', 'release-notes', 'release.md.tmpl'),
      output: outputPath,
    })

    const content = fs.readFileSync(outputPath, 'utf8')
    expect(content).toContain('CLI Downloads')
    expect(content).toContain('uniclipboard-cli-1.2.3-aarch64-apple-darwin.tar.gz')
    expect(content).toContain('uniclipboard-cli-1.2.3-x86_64-unknown-linux-gnu.tar.gz')
    expect(content).toContain('uniclipboard-cli-1.2.3-x86_64-pc-windows-msvc.zip')
    expect(content).toContain('App Installation')
  })

  it('falls back to default text and warns when english changelog is missing', () => {
    const repoDir = setupRepo()
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '1.2.3.zh.md'),
      '## 更新\n\n- 中文',
      'utf8'
    )

    const outputPath = path.join(repoDir, 'release-notes.md')
    const warnings: string[] = []
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(message => {
      warnings.push(String(message))
    })

    generateReleaseNotes({
      version: '1.2.3',
      repo: 'foo/bar',
      previousTag: 'v1.2.2',
      channel: 'stable',
      isPrerelease: false,
      artifactsDir: path.join(repoDir, 'release-assets'),
      template: path.join(repoDir, '.github', 'release-notes', 'release.md.tmpl'),
      output: outputPath,
    })

    warnSpy.mockRestore()

    const content = fs.readFileSync(outputPath, 'utf8')
    expect(content).toContain('Release notes are not available yet.')
    expect(content).toContain('1.2.3.zh.md')
    expect(warnings.some(warning => warning.includes('1.2.3.md'))).toBe(true)
  })
})
