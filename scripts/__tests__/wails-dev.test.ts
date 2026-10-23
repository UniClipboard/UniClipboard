import type { ChildProcess, SpawnOptions, SpawnSyncOptions } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const calls = vi.hoisted(() => ({
  builds: [] as string[],
  app: '',
  vite: null as ChildProcess | null,
  mode: 'normal',
  detached: false,
}))

vi.mock('node:child_process', async importOriginal => {
  const actual = await importOriginal<typeof import('node:child_process')>()
  return {
    ...actual,
    spawnSync(command: string, args: string[], options: SpawnSyncOptions) {
      if (command === 'taskkill') return actual.spawnSync(command, args, options)
      calls.builds.push(command)
      return { status: 0 }
    },
    spawn(command: string, args: string[], options: SpawnOptions) {
      if (command === process.execPath) {
        calls.vite = actual.spawn(command, args, options)
        return calls.vite
      }
      calls.app = command
      calls.detached = options.detached === true
      // Exercise the real Vite HTTP and hot-reload endpoints without opening a native window.
      return actual.spawn(
        process.execPath,
        [
          '-e',
          `
          (async () => {
            if (${JSON.stringify(calls.mode)} === 'tree') {
              const child = require('node:child_process').spawn(process.execPath,
                ['-e', 'setInterval(() => {}, 1000)'], { detached: true, stdio: 'ignore' });
              require('node:fs').writeFileSync(process.env.LAUNCHER_TEST_PID_FILE, String(child.pid));
              setInterval(() => {}, 1000);
              return;
            }
            const url = process.env.FRONTEND_DEVSERVER_URL;
            for (const suffix of ['/', '/@vite/client']) {
              const response = await fetch(url + suffix);
              if (!response.ok) process.exit(2);
            }
            if (process.env.UNICLIPBOARD_ENV !== 'development') process.exit(3);
            if (!process.env.UC_PROFILE) process.exit(4);
            const uiPlatform = { win32: 'windows', linux: 'linux' }[process.platform];
            if (uiPlatform && process.env.UC_UI_PLATFORM !== uiPlatform) process.exit(6);
          })().catch(() => process.exit(5));
        `,
        ],
        options
      )
    },
  }
})

import { main } from '../wails-dev.mjs'

describe('development launcher', () => {
  beforeEach(() => {
    calls.builds = []
    calls.app = ''
    calls.vite = null
    calls.mode = 'normal'
    calls.detached = false
  })
  it.skipIf(!['win32', 'linux'].includes(process.platform))(
    'launches the GUI against Vite and stops the server on exit',
    async () => {
      const root = fileURLToPath(new URL('../../', import.meta.url))
      try {
        expect(
          await main(['--profile', `launcher-test-${process.pid}`], {
            ...process.env,
            ...(process.platform === 'linux' ? { DISPLAY: ':launcher-test' } : {}),
          })
        ).toBe(0)
        expect(calls.builds).toEqual([
          ...(process.platform === 'linux' ? ['pkg-config'] : []),
          'go',
          'cargo',
          'go',
          'go',
        ])
        const binary = process.platform === 'win32' ? 'gui-go-dev.exe' : 'gui-go-dev'
        expect(calls.app).toBe(path.join(root, 'target/gui-go/dev', binary))
        const vite = calls.vite
        if (!vite) throw new Error('Vite was not started')
        await new Promise<void>(resolve => {
          if (vite.exitCode !== null || vite.signalCode !== null) resolve()
          else vite.once('exit', resolve)
        })
        expect(vite.exitCode !== null || vite.signalCode !== null).toBe(true)
      } finally {
        calls.vite?.kill()
      }
    },
    30_000
  )

  it.skipIf(process.platform !== 'win32')(
    'does not launch the GUI after Ctrl+C during Vite startup',
    async () => {
      const initialListeners = process.listenerCount('SIGINT')
      const run = main(['--profile', `launcher-cancel-${process.pid}`])
      try {
        const deadline = Date.now() + 10_000
        while (!calls.vite && Date.now() < deadline) {
          await new Promise(resolve => setTimeout(resolve, 10))
        }
        expect(calls.vite).not.toBeNull()
        process.emit('SIGINT', 'SIGINT')
        expect(await run).toBe(130)
        expect(calls.app).toBe('')
        expect(process.listenerCount('SIGINT')).toBe(initialListeners)
      } finally {
        if (process.listenerCount('SIGINT') > initialListeners) process.emit('SIGINT', 'SIGINT')
        await run
        calls.vite?.kill()
      }
    },
    15_000
  )

  it.skipIf(process.platform !== 'win32')(
    'stops a detached daemon descendant on Ctrl+C and removes signal listeners',
    async () => {
      const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-launcher-test-'))
      const pidFile = path.join(sandbox, 'daemon.pid')
      const initialListeners = process.listenerCount('SIGINT')
      const { spawn } =
        await vi.importActual<typeof import('node:child_process')>('node:child_process')
      const unrelated = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
        windowsHide: true,
        detached: true,
        stdio: 'ignore',
      })
      calls.mode = 'tree'
      let daemonPid = 0
      const run = main(['--profile', `launcher-tree-${process.pid}`], {
        ...process.env,
        LAUNCHER_TEST_PID_FILE: pidFile,
      })
      try {
        const deadline = Date.now() + 20_000
        while (!fs.existsSync(pidFile) && Date.now() < deadline) {
          await new Promise(resolve => setTimeout(resolve, 50))
        }
        expect(fs.existsSync(pidFile)).toBe(true)
        daemonPid = Number(fs.readFileSync(pidFile, 'utf8'))
        expect(() => process.kill(daemonPid, 0)).not.toThrow()
        expect(calls.detached).toBe(true)
        process.emit('SIGINT', 'SIGINT')
        expect(await run).toBe(130)
        expect(() => process.kill(daemonPid, 0)).toThrow()
        expect(() => process.kill(unrelated.pid!, 0)).not.toThrow()
        expect(process.listenerCount('SIGINT')).toBe(initialListeners)
      } finally {
        if (process.listenerCount('SIGINT') > initialListeners) process.emit('SIGINT', 'SIGINT')
        await run
        if (daemonPid) {
          try {
            process.kill(daemonPid)
          } catch {
            /* The launcher already stopped it. */
          }
        }
        unrelated.kill()
        fs.rmSync(sandbox, { recursive: true, force: true })
      }
    },
    30_000
  )
})
