#!/usr/bin/env node
// Kill any stale process listening on the Vite dev-server ports before
// `tauri dev` starts, so repeated dev sessions never collide on port 1420
// ("Port 1420 is already in use").
//
// Why this exists:
// - Tauri CLI polls the fixed `build.devUrl` (http://localhost:1420) and does
//   NOT auto-detect a different Vite port, so `strictPort: false` (auto-increment)
//   is not an option here.
// - A previous dev session that was killed abruptly (IDE restart, `kill -9`,
//   laptop sleep while `tauri dev` was running) leaves the Vite process alive;
//   the next `tauri dev` then hard-fails on the occupied port.
//
// Scope: only the default dev-server ports (1420 and 1421). Profile-specific
// servers fail safely when their port is occupied instead of killing an
// unrelated listener.
// The daemon's own ports are never touched. No-op when nothing is listening.
//
// Platform strategy:
// - macOS / Linux: `lsof -ti tcp:<port>` lists owning PIDs.
// - Windows: `netstat -ano -p tcp` + `taskkill /F /PID`.

import { execFileSync } from 'node:child_process'
import process from 'node:process'

if (process.env.UC_DEV_SERVER_PORT) {
  console.log(`[dev-port-sweep] preserving profile-specific port ${process.env.UC_DEV_SERVER_PORT}`)
  process.exit(0)
}

const DEV_PORTS = [1420, 1421]

function pidsOnPort(port) {
  try {
    if (process.platform === 'win32') {
      const netstat = execFileSync('netstat', ['-ano', '-p', 'tcp'], {
        encoding: 'utf8',
      })
      const pids = new Set()
      for (const line of netstat.split(/\r?\n/)) {
        const match = line.match(/TCP\s+[^:]+:(\d+)\s+\S+\s+LISTENING\s+(\d+)/i)
        if (match && Number(match[1]) === port) {
          pids.add(Number(match[2]))
        }
      }
      return [...pids]
    }
    const out = execFileSync('lsof', ['-ti', `tcp:${port}`], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
    return out
      .split(/\r?\n/)
      .map(line => Number(line.trim()))
      .filter(pid => Number.isFinite(pid) && pid > 0)
  } catch {
    return []
  }
}

function killPid(pid) {
  try {
    if (process.platform === 'win32') {
      execFileSync('taskkill', ['/F', '/PID', String(pid)], { stdio: 'ignore' })
    } else {
      process.kill(pid, 'SIGTERM')
    }
    return true
  } catch {
    return false
  }
}

let killed = 0
for (const port of DEV_PORTS) {
  for (const pid of pidsOnPort(port)) {
    if (killPid(pid)) {
      console.log(`[dev-port-sweep] killed stale dev-server process ${pid} on port ${port}`)
      killed += 1
    }
  }
}

if (killed === 0) {
  console.log('[dev-port-sweep] no stale dev server on ports 1420/1421')
}
