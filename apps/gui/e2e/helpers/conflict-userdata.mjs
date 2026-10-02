import { spawn, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cp, mkdir, readFile, readdir, writeFile, lstat, chmod, realpath } from 'node:fs/promises'
import { homedir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

export const root = resolve('.cache/device-group-e2e')
export function validateTestProfile(profile) {
  if (!/^conflict-e2e-[a-z0-9]+-[a-e]$/.test(profile))
    throw new Error('Not a conflict test profile')
  return profile
}
export const profileDir = profile =>
  join(
    homedir(),
    'Library/Application Support',
    `app.uniclipboard.desktop-${validateTestProfile(profile)}`
  )
const ephemeral = /^(daemon\.conn|\.daemon.*|.*\.lock|.*\.pid|.*-wal|.*-shm)$/
async function validateSnapshot(source) {
  for (const entry of await readdir(source)) {
    const path = join(source, entry)
    const metadata = await lstat(path)
    if (metadata.isSymbolicLink()) throw new Error('Symlink in userdata')
    if (metadata.isDirectory()) await validateSnapshot(path)
    else if (entry.endsWith('-wal') && metadata.size > 0)
      throw new Error('Uncheckpointed database in userdata snapshot')
  }
}
export async function copyUserdata(source, dest) {
  await validateSnapshot(source)
  await mkdir(dest, { recursive: false, mode: 0o700 })
  for (const entry of await readdir(source))
    await cp(join(source, entry), join(dest, entry), {
      recursive: true,
      errorOnExist: true,
      force: false,
      filter: async path => {
        if ((await lstat(path)).isSymbolicLink()) throw new Error('Symlink in userdata')
        return !ephemeral.test(basename(path))
      },
    })
  await protectTree(dest, false)
}
export async function protectTree(path, readonly) {
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const target = join(path, entry.name)
    if (entry.isDirectory()) await protectTree(target, readonly)
    else await chmod(target, readonly ? 0o400 : 0o600)
  }
  await chmod(path, readonly ? 0o500 : 0o700)
}
export async function checksumTree(path, prefix = '') {
  const result = {}
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const name = join(prefix, entry.name)
    if (entry.isDirectory()) Object.assign(result, await checksumTree(join(path, entry.name), name))
    else
      result[name] = createHash('sha256')
        .update(await readFile(join(path, entry.name)))
        .digest('hex')
  }
  return Object.fromEntries(Object.entries(result).sort())
}
export async function waitFor(check, description, timeout = 90000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    try {
      if (await check()) return
    } catch (error) {
      if (error.status !== 503) throw error
    }
    await delay(250)
  }
  throw new Error(`Timed out: ${description}`)
}
export async function connect(profile) {
  const conn = JSON.parse(await readFile(join(profileDir(profile), 'daemon.conn'), 'utf8'))
  const base = `http://${conn.host}:${conn.port}`
  const response = await fetch(`${base}/auth/dev-token?pid=${process.pid}`, {
    method: 'POST',
    signal: AbortSignal.timeout(3000),
  })
  if (!response.ok) throw new Error(`Session unavailable: ${response.status}`)
  const { sessionToken } = await response.json()
  return { profile, pid: conn.pid, base, sessionToken }
}
export async function request(connection, path, method = 'GET', body) {
  if (method !== 'GET') return requestOnce(connection, path, method, body)
  let value
  await waitFor(
    async () => {
      value = await requestOnce(connection, path, method, body)
      return true
    },
    `read ${path}`,
    20000
  )
  return value
}
async function requestOnce(connection, path, method, body) {
  const response = await fetch(`${connection.base}${path}`, {
    method,
    headers: {
      Authorization: `Session ${connection.sessionToken}`,
      'Content-Type': 'application/json',
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(60000),
  })
  const json = await response.json()
  if (!response.ok) {
    const error = new Error(
      `${method} ${path}: HTTP ${response.status}, ${json.code ?? 'request_failed'}`
    )
    error.status = response.status
    throw error
  }
  return json.data
}
async function ownsLiveDaemon(profile, pid) {
  const prefixes = [
    `n${profileDir(profile)}/`,
    `n${join(homedir(), 'Library/Logs', `app.uniclipboard.desktop-${profile}`)}/`,
  ]
  let openFiles
  try {
    openFiles = execFileSync('lsof', ['-p', String(pid), '-Fn'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
  } catch {
    return false
  }
  if (!openFiles.split('\n').some(line => prefixes.some(prefix => line.startsWith(prefix))))
    return false
  let command
  try {
    const state = execFileSync('ps', ['-p', String(pid), '-o', 'stat='], {
      encoding: 'utf8',
    }).trim()
    if (!state || state.startsWith('Z')) return false
    command = execFileSync('ps', ['-p', String(pid), '-o', 'command='], {
      encoding: 'utf8',
    }).trim()
  } catch {
    return false
  }
  const executable = resolve('target/debug/uniclipd')
  const expected = [executable, await realpath(executable)]
  if (!expected.some(path => command === path || command.startsWith(`${path} `)))
    throw new Error('Daemon executable ownership mismatch')
  return true
}
export async function stopProfile(profile, signal = 'SIGTERM') {
  let conn
  try {
    conn = JSON.parse(await readFile(join(profileDir(profile), 'daemon.conn'), 'utf8'))
  } catch {
    return
  }
  if (!(await ownsLiveDaemon(profile, conn.pid))) return
  try {
    if (signal === 'SIGTERM') process.kill(conn.pid, 'SIGCONT')
    process.kill(conn.pid, signal)
  } catch (error) {
    if (error.code === 'ESRCH') return
    throw error
  }
  if (signal === 'SIGTERM')
    await waitFor(
      async () => {
        try {
          process.kill(conn.pid, 0)
          return false
        } catch {
          return true
        }
      },
      'test daemon shutdown',
      20000
    )
}
export function testDaemons() {
  let pids
  try {
    pids = execFileSync('pgrep', ['-x', 'uniclipd'], { encoding: 'utf8' }).trim().split(/\s+/)
  } catch {
    return []
  }
  const result = []
  const prefixes = ['Library/Application Support', 'Library/Logs'].map(
    directory => `n${join(homedir(), directory)}/app.uniclipboard.desktop-`
  )
  for (const pid of pids) {
    let files
    try {
      files = execFileSync('lsof', ['-p', pid, '-Fn'], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      })
    } catch {
      continue
    }
    const name = files
      .split('\n')
      .find(line => prefixes.some(prefix => line.startsWith(prefix + 'conflict-e2e-')))
    const prefix = name && prefixes.find(prefix => name.startsWith(prefix))
    if (name && prefix)
      result.push({
        pid: Number(pid),
        profile: validateTestProfile(name.slice(prefix.length).split('/')[0]),
      })
  }
  return result
}
export async function stopRunProfiles(profiles) {
  profiles.forEach(validateTestProfile)
  for (const profile of profiles) await stopProfile(profile)
  const deadline = Date.now() + 3000
  do {
    for (const daemon of testDaemons().filter(daemon => profiles.includes(daemon.profile))) {
      if (!(await ownsLiveDaemon(daemon.profile, daemon.pid))) continue
      try {
        process.kill(daemon.pid, 'SIGCONT')
        process.kill(daemon.pid, 'SIGTERM')
      } catch (error) {
        if (error.code === 'ESRCH') continue
        throw error
      }
      await waitFor(
        async () => {
          try {
            process.kill(daemon.pid, 0)
            return false
          } catch {
            return true
          }
        },
        'late test daemon shutdown',
        20000
      )
    }
    await delay(250)
  } while (Date.now() < deadline)
  if (testDaemons().some(daemon => profiles.includes(daemon.profile)))
    throw new Error('Test daemon cleanup incomplete')
}
export async function startDaemon(profile) {
  const child = spawn(resolve('target/debug/uniclipd'), [], {
    env: {
      ...process.env,
      UC_PROFILE: validateTestProfile(profile),
      UNICLIPBOARD_ENV: 'development',
      UC_DISABLE_SYSTEM_CLIPBOARD: '1',
      UC_DAEMON_RUN_MODE: 'server',
      RUST_LOG: 'error',
    },
    stdio: 'ignore',
  })
  let connection
  try {
    await waitFor(
      async () => {
        if (child.exitCode !== null) throw new Error(`Test daemon exited: ${child.exitCode}`)
        try {
          connection = await connect(profile)
          return connection.pid === child.pid
        } catch {
          return false
        }
      },
      'test daemon startup',
      30000
    )
  } catch (error) {
    child.kill('SIGTERM')
    throw error
  }
  return connection
}
export async function saveManifest(path, value) {
  await writeFile(path, JSON.stringify(value, null, 2) + '\n', {
    mode: 0o600,
    flag: 'wx',
  })
}
