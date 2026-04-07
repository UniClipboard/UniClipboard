import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { createChangelogDraftFiles, validateChangelogFiles } from '../changelog-draft.js'

describe('changelog-draft', () => {
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
    const repoDir = fs.mkdtempSync(path.join(os.tmpdir(), 'changelog-draft-'))
    tempDirs.push(repoDir)
    process.chdir(repoDir)
    fs.mkdirSync(path.join(repoDir, 'docs', 'changelog'), { recursive: true })
    return repoDir
  }

  it('creates english and chinese changelog drafts for the target version', () => {
    const repoDir = setupRepo()

    createChangelogDraftFiles({
      version: '0.4.0-alpha.6',
      date: '2026-04-07',
    })

    const english = fs.readFileSync(path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.md'))
    const chinese = fs.readFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.zh.md'),
      'utf8'
    )

    expect(String(english)).toContain('## 0.4.0-alpha.6 - 2026-04-07')
    expect(String(english)).toContain('### Features')
    expect(String(english)).toContain('<!--')
    expect(chinese).toContain('## 0.4.0-alpha.6 - 2026-04-07')
    expect(chinese).toContain('### Fixes')
    expect(chinese).toContain('<!--')
  })

  it('fails validation when the chinese changelog is missing', () => {
    const repoDir = setupRepo()
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.md'),
      '## 0.4.0-alpha.6 - 2026-04-07\n\n### Fixes\n\n- Fix startup hang.\n',
      'utf8'
    )

    expect(() =>
      validateChangelogFiles({
        version: '0.4.0-alpha.6',
      })
    ).toThrow('Missing changelog files')
  })

  it('fails validation when placeholder release-note text is still present', () => {
    const repoDir = setupRepo()
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.md'),
      '## 0.4.0-alpha.6 - 2026-04-07\n\nRelease notes are not available yet.\n',
      'utf8'
    )
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.zh.md'),
      '## 0.4.0-alpha.6 - 2026-04-07\n\n### Fixes\n\n- 修复启动卡住的问题。\n',
      'utf8'
    )

    expect(() =>
      validateChangelogFiles({
        version: '0.4.0-alpha.6',
      })
    ).toThrow('unfinished placeholder content')
  })

  it('passes validation once both changelog files contain finalized content', () => {
    const repoDir = setupRepo()
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.md'),
      '## 0.4.0-alpha.6 - 2026-04-07\n\n### Fixes\n\n- Fix startup hang.\n',
      'utf8'
    )
    fs.writeFileSync(
      path.join(repoDir, 'docs', 'changelog', '0.4.0-alpha.6.zh.md'),
      '## 0.4.0-alpha.6 - 2026-04-07\n\n### Fixes\n\n- 修复启动卡住的问题。\n',
      'utf8'
    )

    expect(() =>
      validateChangelogFiles({
        version: '0.4.0-alpha.6',
      })
    ).not.toThrow()
  })
})
