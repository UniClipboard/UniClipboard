import { spawnSync } from 'node:child_process'
import { cpSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { suites } from './suites.mjs'

// Keep executable entry paths and optional exports; assertions have one owner.
export function runEntry(name) {
  const root = fileURLToPath(new URL('../../../../../', import.meta.url))
  const input = process.argv.slice(2)
  let platform = process.env.UC_UI_PLATFORM
  let out
  for (let index = 0; index < input.length; index++) {
    const arg = input[index]
    if (arg === '--platform' || arg === '--out') {
      const value = input[++index]
      if (!value || value.startsWith('--')) throw new Error(`${arg} requires a value`)
      if (arg === '--platform') platform = value
      else out = value
    } else if (['device-tasks', 'history-compact'].includes(name) && !arg.startsWith('--') && !out)
      out = arg
    else throw new Error(`Unknown option: ${arg}`)
  }
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
