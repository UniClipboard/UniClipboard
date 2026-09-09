import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { resolveBuildMode } from '../ci/configure-build-mode.mjs'

describe('package build mode', () => {
  it('leaves release settings and cache identity unchanged', () => {
    expect(resolveBuildMode('release')).toEqual({ cargoEnv: {}, suffix: '' })
  })

  it('overrides only optimization costs for test packages', () => {
    expect(resolveBuildMode('test')).toEqual({
      cargoEnv: {
        CARGO_PROFILE_RELEASE_OPT_LEVEL: '1',
        CARGO_PROFILE_RELEASE_LTO: 'off',
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16',
      },
      suffix: '-test',
    })
  })

  it('rejects unknown modes instead of silently producing another kind of package', () => {
    expect(() => resolveBuildMode('debug')).toThrow('Unknown build mode')
  })

  it.each(['release', 'test'])('writes the actual GitHub command files for %s', mode => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-build-mode-'))
    try {
      const envFile = path.join(directory, 'env')
      const outputFile = path.join(directory, 'output')
      fs.writeFileSync(envFile, 'EXISTING=value\n')
      const result = spawnSync(
        process.execPath,
        [path.resolve(__dirname, '../ci/configure-build-mode.mjs')],
        {
          encoding: 'utf8',
          env: {
            ...process.env,
            UC_BUILD_MODE: mode,
            GITHUB_ENV: envFile,
            GITHUB_OUTPUT: outputFile,
          },
        }
      )
      expect(result.status, result.stderr).toBe(0)
      expect(fs.readFileSync(envFile, 'utf8')).toBe(
        'EXISTING=value\n' +
          Object.entries(resolveBuildMode(mode).cargoEnv)
            .map(([key, value]) => `${key}=${value}\n`)
            .join('')
      )
      expect(fs.readFileSync(outputFile, 'utf8')).toBe(`suffix=${resolveBuildMode(mode).suffix}\n`)
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })
})
