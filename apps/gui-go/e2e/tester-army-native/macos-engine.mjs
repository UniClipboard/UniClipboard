import { spawn, execFile as execFileCallback } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { createInterface } from 'node:readline'
import { promisify } from 'node:util'
import {
  defineEngine,
  ConfigurationError,
  EngineError,
  resolveExpression,
  OBSERVED_NAME_LIMIT,
  OBSERVED_TEXT_LIMIT,
} from 'e2e/engine'

const execFile = promisify(execFileCallback)
export const nativeHistoryTexts = ['Native history adapter alpha', 'Native history adapter beta']

const failure = (code, message, retryable = false) => new EngineError(code, message, { retryable })

export function macosProduct({ scenario = 'history' } = {}) {
  let info, attempt, child, closed, launch, cleanup, session, driverEnv
  let refs = new Map()
  let events = []
  let imageIndex = 0

  async function execute(command, args, context, env = driverEnv) {
    return execFile(command, args, {
      cwd: info.projectRoot,
      env,
      signal: context.signal,
      timeout: Math.max(1, context.timeoutMs),
      maxBuffer: 4 * 1024 * 1024,
    })
  }

  async function desktop(args, context) {
    let result
    try {
      const response = await execute(
        'agent-desktop',
        [...args, ...(session ? ['--session', session] : [])],
        context
      )
      result = JSON.parse(response.stdout)
    } catch (error) {
      if (error.stdout) {
        try {
          result = JSON.parse(error.stdout)
        } catch {
          /* Do not expose raw driver output. */
        }
      }
      if (!result)
        throw failure('ENGINE_FAILURE', 'Native accessibility command did not return JSON')
    }
    if (!result.ok) {
      const error = result.error
      const safe =
        error.disposition?.delivery === 'not_delivered' && error.disposition?.retry === 'safe'
      if (safe && ['STALE_REF', 'SNAPSHOT_NOT_FOUND'].includes(error.code))
        throw failure('NODE_STALE', 'Native accessibility reference expired', true)
      if (!safe) throw failure('ACTION_MAY_HAVE_COMMITTED', `Native command failed: ${error.code}`)
      throw failure('ENGINE_FAILURE', `Native accessibility command failed: ${error.code}`)
    }
    return result.data
  }

  async function verifyWindow(context) {
    if (!launch) throw failure('INVALID_STATE', 'Native product has not launched')
    for (const [pid, expected] of [
      [launch.hostPid, launch.hostIdentity],
      [launch.uiPid, launch.uiIdentity],
    ]) {
      const result = await execute('ps', ['-p', String(pid), '-o', 'lstart=,comm='], context)
      if (result.stdout.trim() !== expected)
        throw failure('INVALID_STATE', 'Owned native process identity changed')
    }
    const parent = await execute('ps', ['-p', String(launch.uiPid), '-o', 'ppid='], context)
    if (Number(parent.stdout.trim()) !== launch.hostPid)
      throw failure('INVALID_STATE', 'Native UI no longer belongs to the owned host')
    const window = await execute(launch.windowInspector, [String(launch.uiPid)], context)
    if (`w-${window.stdout.trim()}` !== launch.windowId)
      throw failure('INVALID_STATE', 'Owned native window identity changed')
  }

  async function snapshot(context) {
    await verifyWindow(context)
    const data = await desktop(
      [
        'snapshot',
        '--window-id',
        launch.windowId,
        '--include-bounds',
        '--max-depth',
        '40',
        '--timeout-ms',
        String(Math.min(context.timeoutMs, 10000)),
      ],
      context
    )
    if (data.window?.id !== launch.windowId || data.app !== launch.appName)
      throw failure('ENGINE_FAILURE', 'Native accessibility observation has a different owner')
    if (!data.complete)
      throw failure('NODE_STALE', 'Native accessibility tree changed during observation', true)
    const bounds = data.tree.bounds
    if (!bounds || bounds.width <= 0 || bounds.height <= 0)
      throw failure('ENGINE_FAILURE', 'Native window has no visible bounds')
    const counts = new Map()
    function count(node) {
      if (node.subtree_truncated || node.children_count > (node.children || []).length)
        throw failure('ENGINE_FAILURE', 'Native accessibility subtree was truncated')
      if (node.native_id) {
        const key = `${node.native_id.kind}:${node.native_id.value}`
        counts.set(key, (counts.get(key) || 0) + 1)
      }
      for (const child of node.children || []) count(child)
    }
    count(data.tree)
    refs = new Map()
    function convert(node, position) {
      const key = node.native_id && `${node.native_id.kind}:${node.native_id.value}`
      const id =
        position === 'root'
          ? 'macos-window'
          : key && counts.get(key) === 1
            ? key
            : `${data.snapshot_id}:${position}`
      if (node.ref_id) refs.set(id, { ref: node.ref_id, actions: node.available_actions || [] })
      const tokens = new Set(node.states || [])
      const secure =
        tokens.has('secure') || tokens.has('protected') || /secure|password/i.test(node.role || '')
      const role =
        {
          radiobutton: 'radio',
          statictext: 'text',
          textfield: 'textbox',
          progressindicator: 'progressbar',
        }[node.role] || node.role
      const states = Object.fromEntries(
        ['checked', 'disabled', 'selected', 'expanded', 'pressed', 'focused', 'hidden']
          .filter(state => tokens.has(state))
          .map(state => [state, true])
      )
      if (secure) states.secure = true
      if (['radio', 'checkbox'].includes(role) && ['0', '1'].includes(node.value))
        states.checked = node.value === '1'
      return {
        ref: { id, revision: '' },
        role,
        ...(node.name ? { name: node.name.slice(0, OBSERVED_NAME_LIMIT) } : {}),
        ...(!secure && node.value !== undefined
          ? { value: String(node.value).slice(0, OBSERVED_TEXT_LIMIT) }
          : {}),
        ...(key ? { testId: node.native_id.value } : {}),
        states,
        ...(node.bounds
          ? {
              rect: {
                x: node.bounds.x - bounds.x,
                y: node.bounds.y - bounds.y,
                width: node.bounds.width,
                height: node.bounds.height,
              },
            }
          : {}),
        children: (node.children || []).map((child, index) =>
          convert(child, `${position}/${index}`)
        ),
      }
    }
    const root = convert(data.tree, 'root')
    const value = {
      location: `native-window:${launch.windowId}`,
      root,
      viewport: { width: bounds.width, height: bounds.height },
    }
    events.push({ kind: 'observe', windowId: launch.windowId, snapshotId: data.snapshot_id })
    return value
  }

  function containsSecure(node) {
    return node.states?.secure || node.children?.some(containsSecure)
  }

  async function capture(label, context, observation) {
    if (containsSecure(observation.root))
      throw failure(
        'UNSUPPORTED_CAPABILITY',
        'Native screenshots with secure controls are not supported'
      )
    await verifyWindow(context)
    const filename = `${++imageIndex}-${(label || 'native-window').replace(/[^a-zA-Z0-9-]/g, '-').slice(0, 80)}.png`
    await execute(
      '/usr/sbin/screencapture',
      ['-x', '-o', '-l', launch.windowId.slice(2), path.join(attempt.artifactsDir, filename)],
      context
    )
    const bytes = await readFile(path.join(attempt.artifactsDir, filename))
    if (!bytes.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex')))
      throw failure('ENGINE_FAILURE', 'Native screenshot is not a PNG')
    events.push({
      kind: 'screenshot',
      windowId: launch.windowId,
      filename,
      width: bytes.readUInt32BE(16),
      height: bytes.readUInt32BE(20),
      sha256: createHash('sha256').update(bytes).digest('hex'),
    })
    return { filename, bytes }
  }

  async function stop(context) {
    if (!child) return
    let sessionError
    if (session) {
      try {
        await desktop(['session', 'end', session], {
          ...context,
          timeoutMs: Math.min(context.timeoutMs, 5000),
        })
      } catch (error) {
        sessionError = error
      }
      session = undefined
    }
    child.stdin.end()
    const result = await closed
    events.push({
      kind: 'exit',
      passed: cleanup?.passed === true,
      hostExitCode: cleanup?.hostExitCode,
      uiExited: cleanup?.uiExited,
      daemonExited: cleanup?.daemonExited,
      homeRemoved: cleanup?.homeRemoved,
    })
    await writeFile(
      path.join(attempt.artifactsDir, 'native-events.json'),
      JSON.stringify(events, null, 2) + '\n'
    )
    child = undefined
    refs.clear()
    if (result.code !== 0 || cleanup?.passed !== true)
      throw failure('ENGINE_FAILURE', 'Native product did not exit normally with verified cleanup')
    if (sessionError) throw sessionError
  }

  return defineEngine({
    name: 'uniclipboard-macos-accessibility',
    version: '1.0.0',
    spiVersion: 1,
    platform: 'macos',
    workers: 1,
    actions: ['tap'],
    validateApp(app, { targetName }) {
      if (process.platform !== 'darwin' || app.appPath !== 'target/gui-go/UniClipboardGoE2E.app')
        throw new ConfigurationError(
          'INVALID_CONFIG',
          `Target ${targetName} requires the isolated macOS E2E product bundle`
        )
    },
    async init(value) {
      info = value
    },
    async startAttempt(context) {
      attempt = context
      events = []
      imageIndex = 0
      cleanup = launch = undefined
      await mkdir(context.artifactsDir, { recursive: true })
      const helper = path.join(
        info.projectRoot,
        'apps/gui-go/e2e/tester-army-native/macos-product.py'
      )
      child = spawn('python3', [helper], {
        cwd: info.projectRoot,
        env: info.env,
        stdio: ['pipe', 'pipe', 'pipe'],
      })
      child.stdin.on('error', () => {})
      closed = new Promise(resolve =>
        child.once('close', (code, signal) => resolve({ code, signal }))
      )
      const ready = new Promise((resolve, reject) => {
        child.once('error', () =>
          reject(failure('ENGINE_FAILURE', 'Native fixture could not start'))
        )
        child.once('close', () => {
          if (!launch) reject(failure('ENGINE_FAILURE', 'Native fixture exited before readiness'))
        })
        createInterface({ input: child.stdout }).on('line', line => {
          let message
          try {
            message = JSON.parse(line)
          } catch {
            reject(failure('ENGINE_FAILURE', 'Invalid native fixture protocol'))
            return
          }
          if (message.phase === 'ready') {
            launch = message
            resolve(message)
          }
          if (message.phase === 'cleanup') cleanup = message
          if (message.phase === 'failure')
            reject(failure('ENGINE_FAILURE', `Native fixture failed: ${message.errorType}`))
        })
      })
      child.stderr.on('data', () => {})
      const abort = () => {
        child.stdin.end()
        if (!launch && child.exitCode === null) child.kill('SIGTERM')
      }
      context.signal.addEventListener('abort', abort, { once: true })
      child.stdin.write(
        JSON.stringify({
          artifactsDir: context.artifactsDir,
          runId: info.runId,
          attemptId: context.attemptId,
          appPath: path.resolve(info.projectRoot, info.app.appPath),
          texts: nativeHistoryTexts,
          scenario,
        }) + '\n'
      )
      try {
        await ready
      } finally {
        context.signal.removeEventListener('abort', abort)
      }
      driverEnv = { ...info.env, AGENT_DESKTOP_HOME: launch.agentHome }
      delete driverEnv.AGENT_DESKTOP_SESSION
      delete driverEnv.AGENT_DESKTOP_AGENT_ID
      const operation = { signal: context.signal, timeoutMs: 15000 }
      const status = await desktop(['status'], operation)
      if (path.resolve(status.state_root) !== path.resolve(launch.agentHome))
        throw failure('INVALID_STATE', 'Native driver state escaped the private profile')
      if (
        status.permissions.accessibility.state !== 'granted' ||
        status.permissions.screen_recording.state !== 'granted'
      )
        throw failure(
          'ENGINE_FAILURE',
          'Native accessibility and screen-recording permissions are required'
        )
      session = (
        await desktop(
          ['session', 'start', '--name', 'uniclipboard-native-history', '--no-trace'],
          operation
        )
      ).session_id
      const binary = (await execute('which', ['agent-desktop'], operation)).stdout.trim()
      const sources = [
        'apps/gui-go/e2e/tester-army-native/macos-engine.mjs',
        'apps/gui-go/e2e/tester-army-native/macos-product.py',
        'apps/gui-go/e2e/tester-army-native/macos-file-transfer.py',
        'apps/gui-go/e2e/tester-army-native/macos-file-transfer.e2e.mjs',
        'e2e.macos-file-transfer.config.ts',
        'apps/gui-go/e2e/tester-army-native/macos-history.e2e.mjs',
        'apps/gui-go/frontend/e2e/tester-army/backend.py',
        'apps/gui-go/e2e/window_id.swift',
        'e2e.macos-history.config.ts',
        'apps/gui-go/e2e/tester-army-native/run-macos-history.mjs',
        'apps/gui-go/build.sh',
        'scripts/gui-e2e.mjs',
        'apps/gui-go/frontend/e2e/tester-army/build-lease.mjs',
        'package.json',
        'bun.lock',
        'Cargo.lock',
      ]
      const hashes = Object.fromEntries(
        await Promise.all(
          sources.map(async file => [
            file,
            createHash('sha256')
              .update(await readFile(path.join(info.projectRoot, file)))
              .digest('hex'),
          ])
        )
      )
      const head = (await execute('git', ['rev-parse', 'HEAD'], operation)).stdout.trim()
      const dirty =
        (await execute('git', ['status', '--porcelain'], operation)).stdout.trim() !== ''
      await writeFile(
        path.join(context.artifactsDir, 'native-driver.json'),
        JSON.stringify(
          {
            head,
            dirty,
            runId: info.runId,
            attemptId: context.attemptId,
            driver: 'agent-desktop',
            version: status.version,
            driverSha256: createHash('sha256')
              .update(await readFile(await realpath(binary)))
              .digest('hex'),
            stateRoot: status.state_root,
            sourceSha256: hashes,
          },
          null,
          2
        ) + '\n'
      )
      events.push({
        kind: 'launch',
        hostPid: launch.hostPid,
        uiPid: launch.uiPid,
        windowId: launch.windowId,
      })
      context.environment({
        platform: 'macos',
        scenario,
        driver: `agent-desktop ${status.version}`,
        nativeWindow: launch.windowId,
      })
      if (scenario === 'file-transfer') child.stdin.write(JSON.stringify({ action: 'send' }) + '\n')
    },
    async observe(context, options) {
      const value = await snapshot(context)
      if (options?.pixels) {
        const { bytes } = await capture('observation', context, value)
        const scale = bytes.readUInt32BE(16) / value.viewport.width
        if (Math.abs(bytes.readUInt32BE(20) / scale - value.viewport.height) > 1)
          throw failure(
            'UNSUPPORTED_CAPABILITY',
            'Native pixels do not align with the accessibility viewport'
          )
        value.pixels = {
          data: bytes,
          mediaType: 'image/png',
          width: bytes.readUInt32BE(16),
          height: bytes.readUInt32BE(20),
          scale,
        }
        value.maskedRegionCount = 0
      }
      return value
    },
    async locate(expression, context) {
      const value = await snapshot(context)
      return resolveExpression(expression, [value.root])
    },
    async perform(ref, action, context) {
      const node = refs.get(ref.id)
      if (!node)
        throw failure('NODE_STALE', 'Native node is no longer in the current observation', true)
      if (action.kind !== 'tap' || !node.actions.includes('Click'))
        throw failure('UNSUPPORTED_CAPABILITY', 'This native node does not expose a semantic click')
      await verifyWindow(context)
      const result = await desktop(['click', node.ref], context)
      events.push({
        kind: 'tap',
        nodeId: ref.id,
        nativeRef: node.ref,
        disposition: result.disposition,
        steps: result.steps,
      })
    },
    artifacts: {
      async screenshot(label, context) {
        const observation = await snapshot(context)
        return (await capture(label, context, observation)).filename
      },
    },
    endAttempt: stop,
    dispose: stop,
  })
}
