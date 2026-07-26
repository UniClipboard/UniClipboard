import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { findUnsupportedJavaScript } from '../check-macos-compat.mjs'

const projectRoot = path.resolve(__dirname, '../..')

function readProjectFile(filePath: string): string {
  try {
    return fs.readFileSync(filePath, 'utf8')
  } catch (error) {
    throw new Error(`Unable to read required project file: ${filePath}`, { cause: error })
  }
}

function parseProjectJson(filePath: string): unknown {
  try {
    return JSON.parse(readProjectFile(filePath))
  } catch (error) {
    throw new Error(`Unable to parse required project JSON: ${filePath}`, { cause: error })
  }
}

function requireJsonObject(value: unknown, description: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error(`Expected ${description} to be a JSON object`)
  }
  return value as Record<string, unknown>
}

describe('macOS 12.5 compatibility guard', () => {
  const tempDirs: string[] = []

  afterEach(() => {
    while (tempDirs.length > 0) {
      const dir = tempDirs.pop()
      if (dir) fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  it('reports positive and negative regular expression lookbehind in built JavaScript', () => {
    const distDir = fs.mkdtempSync(path.join(os.tmpdir(), 'macos-compat-'))
    tempDirs.push(distDir)
    fs.mkdirSync(path.join(distDir, 'assets'))
    fs.writeFileSync(path.join(distDir, 'assets', 'supported.js'), 'const value = /hello/u\n')
    fs.writeFileSync(
      path.join(distDir, 'assets', 'positive.js'),
      'const value = /(?<=hello)world/u\n'
    )
    fs.writeFileSync(
      path.join(distDir, 'assets', 'negative.js'),
      'const value = /(?<!hello)world/u\n'
    )

    expect(findUnsupportedJavaScript(distDir)).toEqual([
      {
        file: path.join(distDir, 'assets', 'negative.js'),
        syntax: 'negative regular expression lookbehind',
      },
      {
        file: path.join(distDir, 'assets', 'positive.js'),
        syntax: 'positive regular expression lookbehind',
      },
    ])
  })

  it('keeps the build and bundle configuration aligned with macOS 12.5', () => {
    const packageJson = requireJsonObject(
      parseProjectJson(path.join(projectRoot, 'package.json')),
      'package.json'
    )
    const packageScripts = requireJsonObject(packageJson.scripts, 'package.json scripts')
    const packageDependencies = requireJsonObject(
      packageJson.dependencies,
      'package.json dependencies'
    )
    const tauriConfig = requireJsonObject(
      parseProjectJson(path.join(projectRoot, 'src-tauri', 'tauri.conf.json')),
      'tauri.conf.json'
    )
    const tauriBundle = requireJsonObject(tauriConfig.bundle, 'tauri.conf.json bundle')
    const macOSBundle = requireJsonObject(tauriBundle.macOS, 'tauri.conf.json macOS bundle')
    const viteConfig = readProjectFile(path.join(projectRoot, 'vite.config.ts'))
    const releaseNotes = readProjectFile(
      path.join(projectRoot, 'src', 'components', 'update', 'ReleaseNotes.tsx')
    )

    expect(packageScripts.build).toContain('node scripts/check-macos-compat.mjs')
    expect(packageDependencies).not.toHaveProperty('remark-gfm')
    expect(macOSBundle.minimumSystemVersion).toBe('12.5')
    expect(viteConfig).toMatch(/target:\s*'safari15\.6'/)
    expect(viteConfig).toMatch(/cssTarget:\s*'safari15\.6'/)
    expect(releaseNotes).not.toContain('remark-gfm')
  })
})
