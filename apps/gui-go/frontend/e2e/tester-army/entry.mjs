import { spawnSync } from 'node:child_process'
import { cpSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { suites } from './suites.mjs'

// Keep executable entry paths and optional exports; assertions have one owner.
export function runEntry(name) {
  const root = fileURLToPath(new URL('../../../../../', import.meta.url))
  const input = process.argv.slice(2)
  const option = key => (input.includes(key) ? input[input.indexOf(key) + 1] : undefined)
  const platform = option('--platform') || process.env.UC_UI_PLATFORM
  const out =
    option('--out') ||
    (name === 'device-tasks' ? input.find(arg => !arg.startsWith('--')) : undefined)
  const args = ['apps/gui-go/frontend/e2e/tester-army/run-layouts.mjs', name]
  const skins = suites[name].platforms.includes(platform) ? [platform] : suites[name].platforms
  if (skins.length === 1) args.push('--platform', skins[0])
  const result = spawnSync(process.execPath, args, {
    cwd: root,
    stdio: 'inherit',
    env: process.env,
  })
  if (result.error) throw result.error
  if (result.status === 0 && out) {
    const destination = path.resolve(out)
    mkdirSync(destination, { recursive: true })
    for (const skin of skins)
      cpSync(
        path.join(root, 'target/tester-army-layout', skin, name),
        skins.length === 1 ? destination : path.join(destination, skin),
        { recursive: true }
      )
  }
  process.exitCode = result.status ?? 1
}
