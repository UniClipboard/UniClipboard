import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

// Sentry/OTLP environment tag baked into binaries at compile time (APP_ENV via
// option_env!) and inlined into the frontend (VITE_APP_ENV). Shared by the app
// and CLI builds so every shipped `uniclipd` reports the same environment.
export function resolveAppEnv(channel) {
  switch (channel || 'stable') {
    case 'stable':
      return { appEnv: 'production', known: true }
    case 'alpha':
    case 'beta':
    case 'rc':
      return { appEnv: channel, known: true }
    default:
      return { appEnv: 'production', known: false }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const channel = process.env.UC_CHANNEL ?? ''
  const { appEnv, known } = resolveAppEnv(channel)
  if (!process.env.GITHUB_OUTPUT) {
    throw new Error('GITHUB_OUTPUT is required')
  }
  if (!known) {
    console.log(`::warning::Unknown channel '${channel}', falling back to production`)
  }
  appendFileSync(process.env.GITHUB_OUTPUT, `app_env=${appEnv}\n`)
  console.log(`Resolved APP_ENV=${appEnv} (from channel=${channel || 'stable'})`)
}
