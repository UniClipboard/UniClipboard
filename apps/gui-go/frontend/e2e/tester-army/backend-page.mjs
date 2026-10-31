import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdir, writeFile } from 'node:fs/promises'
import { createInterface } from 'node:readline'
import { surfaceOf, test as base } from '@e2e-dev/web'
import { redactEvidence } from '../evidence-redaction.mjs'
import { backendEngine } from './backend-engine.mjs'
import { backendOutput, backendSeed } from './backend-suites.mjs'

export async function installSession(page, daemon, token = daemon.sessionToken) {
  await page.addInitScript(
    ({ baseUrl, sessionToken }) => {
      window.__ucBrowserDaemon = { baseUrl, sessionToken }
    },
    { baseUrl: daemon.baseUrl, sessionToken: token }
  )
}
export const test = base.extend({
  backend: async (_, provide) => {
    await mkdir(backendOutput, { recursive: true })
    const proc = spawn(
      'python3',
      ['apps/gui-go/frontend/e2e/tester-army/backend.py', backendSeed],
      {
        stdio: ['pipe', 'pipe', 'pipe'],
      }
    )
    const closed = once(proc, 'close')
    const lines = createInterface({ input: proc.stdout })
    const drained = once(lines, 'close')
    let cleanup
    let failed
    const diagnostics = []
    let rejectedDiagnostics = 0
    const stderr = createInterface({ input: proc.stderr })
    const stderrDrained = once(stderr, 'close')
    stderr.on('line', line => {
      // Store only the helper's finite diagnostic schema, never arbitrary stderr.
      try {
        const value = JSON.parse(line)
        if (
          ![
            'stage',
            'command-start',
            'command-exit',
            'command-failed',
            'failure',
            'cleanup-failed',
          ].includes(value.event) ||
          ![
            'profile',
            'space-init',
            'daemon-start',
            'connection',
            'authentication',
            'unlock',
            'settings',
            'ready',
            'cleanup',
            'seed-empty',
            'seed-panel',
            'seed-devices',
            'seed-history',
            'seed-search',
            'seed-smoke',
          ].includes(value.stage)
        )
          throw new Error('unexpected diagnostic schema')
        const safe = { event: value.event, stage: value.stage }
        if (['space', 'start', 'stop', 'send', 'mobile'].includes(value.operation))
          safe.operation = value.operation
        if (Number.isInteger(value.returnCode)) safe.returnCode = value.returnCode
        if (Number.isInteger(value.errno)) safe.errno = value.errno
        if (
          [
            'RuntimeError',
            'AssertionError',
            'TimeoutExpired',
            'FileNotFoundError',
            'PermissionError',
            'HTTPError',
            'URLError',
            'JSONDecodeError',
            'ValueError',
            'OSError',
            'TimeoutError',
          ].includes(value.errorType)
        )
          safe.errorType = value.errorType
        diagnostics.push(safe)
        if (diagnostics.length > 200) diagnostics.shift()
      } catch {
        rejectedDiagnostics++
      }
    })
    let timer
    try {
      const ready = await Promise.race([
        new Promise(resolve =>
          lines.on('line', line => {
            try {
              const value = JSON.parse(line)
              if (value.phase === 'cleanup') cleanup = value
              else if (typeof value.baseUrl === 'string' && typeof value.sessionToken === 'string')
                resolve(value)
              else rejectedDiagnostics++
            } catch {
              rejectedDiagnostics++
            }
          })
        ),
        closed.then(([code]) => {
          throw new Error(`backend helper exited ${code}`)
        }),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('backend bootstrap timed out')), 180_000)
        }),
      ])
      clearTimeout(timer)
      await provide(ready)
    } catch (error) {
      failed = error
      throw error
    } finally {
      clearTimeout(timer)
      if (!proc.stdin.destroyed) proc.stdin.end()
      let cleanupTimer
      let outcome
      let cleanupError
      try {
        outcome = await Promise.race([
          closed,
          new Promise((_, reject) => {
            cleanupTimer = setTimeout(
              () =>
                reject(
                  new Error(
                    'owned backend cleanup exceeded 105 seconds; sandbox ownership is unresolved'
                  )
                ),
              105000
            )
          }),
        ])
        await Promise.all([drained, stderrDrained])
      } catch (error) {
        cleanupError = error.message
      } finally {
        clearTimeout(cleanupTimer)
      }
      const code = outcome?.[0] ?? null
      await writeFile(
        `${backendOutput}/isolation.json`,
        JSON.stringify(
          { exitCode: code, cleanupError, ...cleanup, diagnostics, rejectedDiagnostics },
          null,
          2
        ) + '\n'
      )
      if (!failed) {
        assert.equal(code, 0, 'isolated backend must exit cleanly')
        for (const key of ['homeRemoved', 'daemonExited', 'systemClipboardDisabled', 'guiIsolated'])
          assert.equal(cleanup?.[key], true, `backend ${key}`)
      }
    }
  },
  page: async ({ app }, provide) => {
    await app.open('/runner-blank.html')
    await provide(surfaceOf(backendEngine).page())
  },
  createContext: async ({ backend }, provide) => {
    const contexts = []
    const observations = []
    const redact = text => {
      for (const secret of [backend.fileToken, backend.sessionToken, backend.passphrase])
        text = text.replaceAll(secret, '[redacted]')
      return redactEvidence(text)
    }
    try {
      await provide(async options => {
        const context = await surfaceOf(backendEngine).context().browser().newContext(options)
        contexts.push(context)
        context.on('page', page => {
          page.on('websocket', socket =>
            socket.on('framereceived', frame => {
              try {
                const data = JSON.parse(String(frame.payload))
                observations.push({
                  type: 'ws',
                  kind: data.type,
                  topic: data.topic,
                  topics: data.payload?.topics ?? data.topics,
                  event: typeof data.event === 'string' ? data.event : undefined,
                })
              } catch {
                /* Binary transport data contains no diagnostic event. */
              }
            })
          )
          page.on('console', message =>
            observations.push({ type: message.type(), text: redact(message.text()) })
          )
          page.on('response', response => {
            if (response.status() >= 400)
              observations.push({
                type: 'http',
                path: new URL(response.url()).pathname,
                status: response.status(),
              })
          })
        })
        return context
      })
    } finally {
      for (const context of contexts) await context.close()
      await writeFile(
        `${backendOutput}/observations.json`,
        JSON.stringify(observations, null, 2) + '\n'
      )
    }
  },
})
