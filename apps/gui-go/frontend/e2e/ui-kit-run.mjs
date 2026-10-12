// Entry point for the platform UI kit gallery and its browser E2E, with an explicit target:
//   node e2e/ui-kit-run.mjs --platform windows|macos|linux [--serve] [--port 1460]
// The platform is passed to the gallery build as UC_UI_PLATFORM and to the E2E as
// UI_KIT_PLATFORM, which checks that the served skin and the build marker match it. `--serve`
// only builds and serves the gallery (for looking at it by hand). Playwright comes from
// PLAYWRIGHT_MODULE when it is not installed in the repository, as for the other browser checks.
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const frontend = path.resolve(here, '..')
const args = process.argv.slice(2)
const option = name => {
  const index = args.indexOf(name)
  return index >= 0 ? args[index + 1] : undefined
}
const platform = option('--platform')
// A present --port needs a valid value; only an absent flag falls back to the default.
const port = args.includes('--port') ? option('--port') : '1460'
if (!/^[1-9][0-9]{0,4}$/.test(port ?? '') || Number(port) > 65535) {
  console.error(
    `--port needs a TCP port number (got ${port === undefined ? 'nothing' : `"${port}"`})`
  )
  process.exit(2)
}
const serveOnly = args.includes('--serve')
if (!['windows', 'macos', 'linux'].includes(platform ?? '')) {
  console.error(
    'Usage: node e2e/ui-kit-run.mjs --platform windows|macos|linux [--serve] [--port 1460]'
  )
  process.exit(2)
}

const server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
  cwd: frontend,
  env: {
    ...process.env,
    UC_UI_PLATFORM: platform,
    UI_FIXTURE_ENTRY: 'e2e/fixtures/ui-kit-gallery.tsx',
    UI_FIXTURE_PORT: port,
  },
  stdio: ['ignore', 'pipe', 'inherit'],
})
const stop = () => server.exitCode === null && server.kill()
process.on('SIGINT', () => {
  stop()
  process.exit(130)
})

const ready = new Promise((resolve, reject) => {
  server.stdout.on('data', chunk => {
    process.stdout.write(chunk)
    if (String(chunk).includes('Local:')) resolve()
  })
  server.on('exit', code => reject(new Error(`the gallery server exited with code ${code}`)))
})
try {
  await ready
} catch (error) {
  console.error(error.message)
  process.exit(1)
}
if (serveOnly) {
  console.log(`[ui-kit] ${platform} gallery at http://127.0.0.1:${port}/ (Ctrl+C to stop)`)
} else {
  const e2e = spawn(process.execPath, ['e2e/ui-kit-browser.mjs'], {
    cwd: frontend,
    env: {
      ...process.env,
      UI_KIT_PLATFORM: platform,
      UI_KIT_TEST_URL: `http://127.0.0.1:${port}`,
    },
    stdio: 'inherit',
  })
  const code = await new Promise(resolve => e2e.on('exit', resolve))
  stop()
  process.exit(code ?? 1)
}
