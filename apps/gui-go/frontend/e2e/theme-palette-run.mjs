// Serial browser acceptance; stop each server before building the next platform.
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
const cwd = fileURLToPath(new URL('..', import.meta.url))
for (const platform of ['linux', 'macos', 'windows']) {
  const env = {
    ...process.env,
    UC_UI_PLATFORM: platform,
    UI_FIXTURE_ENTRY: 'e2e/fixtures/theme-palette.tsx',
    UI_FIXTURE_PORT: '1462',
  }
  const server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
    cwd,
    env,
    stdio: ['ignore', 'pipe', 'inherit'],
  })
  try {
    await new Promise((resolve, reject) => {
      server.stdout.on('data', chunk => {
        if (String(chunk).includes('Local:')) resolve()
      })
      server.on('exit', code => reject(new Error(`${platform} fixture server exited: ${code}`)))
    })
    const test = spawn(process.execPath, ['e2e/theme-palette-browser.mjs'], {
      cwd,
      env,
      stdio: 'inherit',
    })
    const code = await new Promise(resolve => test.on('exit', resolve))
    if (code !== 0) throw new Error(`${platform} palette acceptance failed`)
  } finally {
    const stopped = new Promise(resolve => server.once('exit', resolve))
    if (server.exitCode === null && server.signalCode === null) {
      server.kill()
      await stopped
    }
  }
}
