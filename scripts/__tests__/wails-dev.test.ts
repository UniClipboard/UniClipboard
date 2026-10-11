import type { ChildProcess, SpawnOptions } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'

const calls = vi.hoisted(() => ({
  builds: [] as string[],
  app: '',
  vite: null as ChildProcess | null,
}))

vi.mock('node:child_process', async importOriginal => {
  const actual = await importOriginal<typeof import('node:child_process')>()
  return {
    ...actual,
    spawnSync(command: string) {
      calls.builds.push(command)
      return { status: 0 }
    },
    spawn(command: string, args: string[], options: SpawnOptions) {
      if (command === process.execPath) {
        calls.vite = actual.spawn(command, args, options)
        return calls.vite
      }
      calls.app = command
      // Exercise the real Vite HTTP and hot-reload endpoints without opening a native window.
      return actual.spawn(
        process.execPath,
        [
          '-e',
          `
          (async () => {
            const url = process.env.FRONTEND_DEVSERVER_URL;
            for (const suffix of ['/', '/@vite/client']) {
              const response = await fetch(url + suffix);
              if (!response.ok) process.exit(2);
            }
            if (process.env.UNICLIPBOARD_ENV !== 'development') process.exit(3);
            if (!process.env.UC_PROFILE) process.exit(4);
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
  it.skipIf(process.platform !== 'win32')(
    'launches an exe against Vite on Windows and stops the server on exit',
    async () => {
      const root = fileURLToPath(new URL('../../', import.meta.url))
      try {
        expect(await main(['--profile', `launcher-test-${process.pid}`])).toBe(0)
        expect(calls.builds).toEqual(['cargo', 'go', 'go'])
        expect(calls.app).toBe(path.join(root, 'target/gui-go/dev/gui-go-dev.exe'))
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
})
