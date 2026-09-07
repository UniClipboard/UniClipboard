import { readFile, readdir, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { validateTestProfile } from './conflict-userdata.mjs'

export function publicEngineEvent(event, role) {
  if (
    typeof event.target !== 'string' ||
    !/^uc_(engine|application|infra|platform)::/.test(event.target)
  )
    return null
  const symbol = value =>
    typeof value === 'string' && /^[a-zA-Z0-9_:.-]+$/.test(value) ? value : undefined
  const timestamp =
    typeof event.timestamp === 'string' && /^\d{4}-\d{2}-\d{2}T[\d:.]+Z$/.test(event.timestamp)
      ? event.timestamp
      : undefined
  if (!timestamp) return null
  return {
    timestamp,
    role,
    level: symbol(event.level),
    target: symbol(event.target),
    span: symbol(event.span),
    error_kind: symbol(event.error_kind),
  }
}

export async function collectEngineEvidence(run) {
  const events = []
  const unavailable = []
  for (const [index, profile] of run.profiles.entries()) {
    validateTestProfile(profile)
    const role = 'abcde'[index]
    const directory = join(homedir(), 'Library/Logs', `app.uniclipboard.desktop-${profile}`)
    try {
      for (const file of await readdir(directory)) {
        if (!file.startsWith('uniclipboard-daemon.json.')) continue
        for (const line of (await readFile(join(directory, file), 'utf8')).split('\n')) {
          if (!line) continue
          try {
            const event = publicEngineEvent(JSON.parse(line), role)
            if (event) events.push(event)
          } catch {
            /* A running logger can leave its final line incomplete. */
          }
        }
      }
    } catch (error) {
      unavailable.push({ role, code: error.code ?? 'unavailable' })
    }
  }
  events.sort((left, right) => left.timestamp.localeCompare(right.timestamp))
  await writeFile(
    join(run.runDir, 'engine-events.jsonl'),
    events.map(event => JSON.stringify(event)).join('\n') + '\n',
    { mode: 0o600 }
  )
  await writeFile(
    join(run.runDir, 'engine-evidence.json'),
    JSON.stringify({ eventCount: events.length, unavailable }, null, 2),
    { mode: 0o600 }
  )
}
