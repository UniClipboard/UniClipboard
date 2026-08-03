import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { describe, expect, it } from 'vitest'
import * as launcher from '../tauri-dev-profile.mjs'

const projectRoot = path.resolve(__dirname, '../..')
const scriptPath = path.join(projectRoot, 'scripts/tauri-dev-profile.mjs')

type Invocation = {
  command: string
  args: string[]
  env: NodeJS.ProcessEnv
}

function createInvocation(args: string[], env: NodeJS.ProcessEnv = {}): Invocation {
  const createTauriDevInvocation = Reflect.get(launcher, 'createTauriDevInvocation') as unknown
  expect(createTauriDevInvocation).toBeTypeOf('function')
  if (typeof createTauriDevInvocation !== 'function') throw new Error('missing launcher function')

  return createTauriDevInvocation(args, env) as Invocation
}

describe('custom-profile Tauri development command', () => {
  it('is exposed through package.json', () => {
    const packageJson = JSON.parse(
      fs.readFileSync(path.join(projectRoot, 'package.json'), 'utf8')
    ) as { scripts: Record<string, string> }

    expect(packageJson.scripts['tauri:dev:profile']).toBe('node scripts/tauri-dev-profile.mjs')
    expect(fs.existsSync(path.join(projectRoot, 'scripts/tauri-dev-profile.mjs'))).toBe(true)
  })

  it('keeps daemon single-instance protection enabled in every development command', () => {
    const packageJson = JSON.parse(
      fs.readFileSync(path.join(projectRoot, 'package.json'), 'utf8')
    ) as { scripts: Record<string, string> }

    const developmentCommands = Object.entries(packageJson.scripts).filter(([name]) =>
      name.startsWith('tauri:dev')
    )
    for (const [, command] of developmentCommands) {
      expect(command).not.toContain('UC_DISABLE_SINGLE_INSTANCE=1')
    }
  })

  it('requires a profile name', () => {
    expect(() => createInvocation([])).toThrow(/profile name is required/i)
  })

  it.each([' ', '../alice', 'alice/bob', 'alice\\bob', '-alice', 'alice.profile'])(
    'rejects unsafe profile name %j',
    profile => {
      expect(() => createInvocation([profile])).toThrow(/invalid profile name/i)
    }
  )

  it('sets the development profile and preserves the existing environment', () => {
    const invocation = createInvocation(['alice'], { EXISTING_VALUE: 'kept' })

    expect(invocation).toEqual({
      command: 'bun',
      args: ['run', 'tauri', '--', 'dev'],
      env: {
        EXISTING_VALUE: 'kept',
        UNICLIPBOARD_ENV: 'development',
        UC_PROFILE: 'alice',
      },
    })
  })

  it('removes an optional separator and forwards Tauri arguments in order', () => {
    const invocation = createInvocation(['alice', '--', '--no-dev-server-wait', '--config', '{}'])

    expect(invocation.args).toEqual([
      'run',
      'tauri',
      '--',
      'dev',
      '--no-dev-server-wait',
      '--config',
      '{}',
    ])
  })

  it('prints usage without starting Tauri when help is requested', () => {
    const result = spawnSync(process.execPath, [scriptPath, '--help'], { encoding: 'utf8' })

    expect(result.status).toBe(0)
    expect(result.stdout).toContain('bun tauri:dev:profile <profile>')
    expect(result.stderr).toBe('')
  })

  it('prints an error and usage when the profile is missing', () => {
    const result = spawnSync(process.execPath, [scriptPath], { encoding: 'utf8' })

    expect(result.status).toBe(1)
    expect(result.stderr).toMatch(/profile name is required/i)
    expect(result.stderr).toContain('bun tauri:dev:profile <profile>')
  })
})
