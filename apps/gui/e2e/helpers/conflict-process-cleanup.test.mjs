import { realpath } from 'node:fs/promises'
import { resolve } from 'node:path'
import { afterEach, expect, it, vi } from 'vitest'
import { profileDir, stopRunProfiles } from './conflict-userdata.mjs'

const { exec, read } = vi.hoisted(() => ({ exec: vi.fn(), read: vi.fn() }))
vi.mock('node:child_process', async original => {
  const actual = await original()
  return {
    ...actual,
    execFileSync: exec,
    default: { ...actual.default, execFileSync: exec },
  }
})

it.each(['workspace', 'canonical', 'foreign'])(
  'checks the exact executable identity for a live %s process',
  async kind => {
    const profile = 'conflict-e2e-cleanup-a'
    const executable = resolve('target/debug/uniclipd')
    const command =
      kind === 'canonical'
        ? await realpath(executable)
        : kind === 'foreign'
          ? `/unrelated${executable}`
          : executable
    let enumerations = 0
    read.mockRejectedValue(Object.assign(new Error('gone'), { code: 'ENOENT' }))
    exec.mockImplementation((tool, args) => {
      if (tool === 'pgrep') {
        if (enumerations++ === 0) return '4242\n'
        throw new Error('no process')
      }
      if (tool === 'lsof') return `p4242\nn${profileDir(profile)}/control.sqlite\n`
      if (args.includes('stat=')) return 'S\n'
      return command
    })
    vi.spyOn(Date, 'now').mockReturnValueOnce(0).mockReturnValue(4000)
    const signal = vi.spyOn(process, 'kill').mockImplementation((_pid, value) => {
      if (value === 0) throw Object.assign(new Error('gone'), { code: 'ESRCH' })
      return true
    })
    if (kind === 'foreign') {
      await expect(stopRunProfiles([profile])).rejects.toThrow('ownership mismatch')
      expect(signal).not.toHaveBeenCalled()
    } else {
      await stopRunProfiles([profile])
      expect(signal).toHaveBeenCalledWith(4242, 'SIGTERM')
    }
  }
)
vi.mock('node:fs/promises', async original => {
  const actual = await original()
  const resolveExecutable = async () => '/test/canonical/uniclipd'
  return {
    ...actual,
    readFile: read,
    realpath: resolveExecutable,
    default: { ...actual.default, readFile: read, realpath: resolveExecutable },
  }
})
afterEach(() => vi.restoreAllMocks())

it('does not signal a process that exited after its test files were observed', async () => {
  const profile = 'conflict-e2e-cleanup-a'
  let enumerations = 0
  read.mockRejectedValue(Object.assign(new Error('gone'), { code: 'ENOENT' }))
  exec.mockImplementation((command, args) => {
    if (command === 'pgrep') {
      if (enumerations++ === 0) return '4242\n'
      throw new Error('no process')
    }
    if (command === 'lsof') return `p4242\nn${profileDir(profile)}/control.sqlite\n`
    if (command === 'ps' && args.includes('stat=')) return 'Z\n'
    if (command === 'ps') return '(uniclipd)\n'
    throw new Error('unexpected process query')
  })
  vi.spyOn(Date, 'now').mockReturnValueOnce(0).mockReturnValue(4000)
  const signal = vi.spyOn(process, 'kill').mockImplementation(() => true)
  await expect(stopRunProfiles([profile])).resolves.toBeUndefined()
  expect(signal).not.toHaveBeenCalled()
})
