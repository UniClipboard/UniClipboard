import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

const matrix = [
  ['four', 'apply'],
  ['four', 'cross-local'],
  ['four', 'cross-remote'],
  ['four', 'disagreement'],
  ['five', 'local-remove'],
  ['four', 'new-peer'],
  ['four', 'disconnect'],
  ['four', 'restart-cycle'],
  ['four', 'response-loss'],
  ['four', 'apply'],
  ['five', 'local-remove'],
  ['four', 'controlled'],
  ['four', 'native-keyboard'],
]
const directory = join('.cache/device-group-e2e', `suite-${Date.now().toString(36)}`)
await mkdir(directory, { recursive: true, mode: 0o700 })
const results = []
for (const [index, [baseline, scenario]] of matrix.entries()) {
  console.log(`Scenario ${index + 1}/${matrix.length}: ${baseline} ${scenario}`)
  const child = spawn(process.execPath, ['e2e/conflict-suite.mjs', baseline, scenario], {
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let output = ''
  child.stdout.on('data', chunk => {
    output += chunk
  })
  child.stderr.on('data', chunk => {
    output += chunk
  })
  const exit = await new Promise(resolve => child.once('exit', resolve))
  const log = `${index + 1}-${baseline}-${scenario}.log`
  await writeFile(join(directory, log), output, { mode: 0o600 })
  results.push({
    baseline,
    scenario,
    exit,
    log,
    manifest: output.match(/Run manifest: (.+)/)?.[1] ?? null,
  })
  await writeFile(join(directory, 'results.json'), JSON.stringify(results, null, 2))
  console.log(`${exit === 0 ? 'PASS' : 'FAIL'} ${scenario}`)
}
console.log(`Suite report: ${directory}/results.json`)
process.exitCode = results.some(result => result.exit !== 0) ? 1 : 0
