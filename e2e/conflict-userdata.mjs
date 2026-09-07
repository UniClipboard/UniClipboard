import { execFileSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { mkdir, readFile, rename, rmdir, unlink } from 'node:fs/promises'
import { join, resolve, relative } from 'node:path'
import {
  root,
  profileDir,
  copyUserdata,
  checksumTree,
  startDaemon,
  stopProfile,
  request,
  waitFor,
  saveManifest,
  protectTree,
  stopRunProfiles,
} from './helpers/conflict-userdata.mjs'

const [action, name = 'four'] = process.argv.slice(2)
if (!['four', 'five'].includes(name)) throw new Error('Use four or five')
await mkdir(root, { recursive: true, mode: 0o700 })
const baseline = join(root, name)
const lock = join(root, `${name}.lock`)
if (action === 'generate') {
  await mkdir(lock)
  const run = Date.now().toString(36)
  const profiles = 'abcde'
    .slice(0, name === 'four' ? 4 : 5)
    .split('')
    .map(role => `conflict-e2e-${run}-${role}`)
  const staging = join(root, `${name}-${run}`)
  await mkdir(staging)
  try {
    const nodes = []
    const passphrase = randomBytes(24).toString('hex')
    for (const [index, profile] of profiles.entries()) {
      const node = await startDaemon(profile)
      nodes.push(node)
      await request(node, '/settings', 'PUT', {
        general: {
          deviceName: `Test ${'ABCDE'[index]}`,
          autoCheckUpdate: false,
          usageAnalyticsEnabled: false,
          telemetryEnabled: false,
        },
      })
      if (index === 0)
        await request(node, '/v2/setup/initialize', 'POST', {
          passphrase,
          passphraseConfirm: passphrase,
          deviceName: 'Test A',
        })
      else {
        const invitation = await request(nodes[0], '/v2/setup/issue-invitation', 'POST')
        const admission = await request(node, '/v2/setup/redeem', 'POST', {
          code: invitation.code,
          passphrase,
        })
        if (admission.status === 'rejected') throw new Error(`Join rejected: ${admission.reason}`)
        await waitFor(
          async () => {
            const group = await request(node, '/member/device-group-choices')
            return group.deviceTrust.currentJoin?.status === 'active'
          },
          'join activation',
          150000
        )
      }
      await waitFor(
        async () => {
          for (const current of nodes) {
            const group = await request(current, '/member/device-group-choices')
            if (
              group.deviceTrust.devices.filter(d => d.membership === 'active').length !==
              nodes.length
            )
              return false
          }
          return true
        },
        'complete roster',
        150000
      )
      console.log(`Prepared ${index + 1}/${profiles.length} independent profiles`)
    }
    const expected = []
    for (const node of nodes)
      await waitFor(
        async () => {
          const group = await request(node, '/member/device-group-choices')
          if (
            group.issues.length ||
            group.deviceTrust.devices.some(
              device =>
                device.groupRelationship !== 'consistent' || device.syncRelationship !== 'usable'
            ) ||
            group.deviceTrust.devices.filter(d => d.membership === 'active').length !==
              profiles.length
          )
            return false
          expected.push(group)
          return true
        },
        'final baseline snapshot',
        150000
      )
    for (const profile of profiles) await stopProfile(profile)
    for (const [i, profile] of profiles.entries())
      await copyUserdata(profileDir(profile), join(staging, 'abcde'[i]))
    const files = await checksumTree(staging)
    await saveManifest(join(staging, 'manifest.json'), {
      version: 1,
      name,
      profiles,
      passphrase,
      createdAt: new Date().toISOString(),
      engine: execFileSync('git', ['-C', '../Engine', 'rev-parse', 'HEAD'], {
        encoding: 'utf8',
      }).trim(),
      expected,
      files,
    })
    // Never replace an existing baseline implicitly.
    await mkdir(baseline)
    await rmdir(baseline)
    await rename(staging, baseline)
    await protectTree(baseline, true)
    console.log(`Baseline ready: ${name}`)
  } finally {
    for (const profile of profiles) await stopProfile(profile)
    await rmdir(lock)
  }
} else if (action === 'archive') {
  await mkdir(lock)
  try {
    await rename(baseline, join(root, `${name}-archived-${Date.now().toString(36)}`))
  } finally {
    await rmdir(lock)
  }
  console.log('Previous test baseline archived without deleting it')
} else if (action === 'restore' || action === 'restore-failed') {
  await mkdir(lock)
  try {
    const manifest = JSON.parse(await readFile(join(baseline, 'manifest.json'), 'utf8'))
    let sourceRun = null
    if (action === 'restore-failed') {
      const sourcePath = process.argv[4] && resolve(process.argv[4])
      if (!sourcePath?.startsWith(root + '/run-') || !sourcePath.endsWith('/run.json'))
        throw new Error('Recovery requires a retained test run manifest')
      sourceRun = JSON.parse(await readFile(sourcePath, 'utf8'))
      if (sourceRun.baseline !== name || ![4, 5].includes(sourceRun.profiles?.length))
        throw new Error('Recovery source baseline mismatch')
      sourceRun.profiles.forEach(profileDir)
    }
    if (manifest.version !== 1)
      throw new Error('Unsupported baseline version; regenerate explicitly')
    const engine = execFileSync('git', ['-C', '../Engine', 'rev-parse', 'HEAD'], {
      encoding: 'utf8',
    }).trim()
    if (!sourceRun && engine !== manifest.engine)
      throw new Error('Engine changed; validate migration or regenerate the baseline')
    const actual = await checksumTree(baseline)
    delete actual['manifest.json']
    if (JSON.stringify(actual) !== JSON.stringify(manifest.files))
      throw new Error('Baseline checksum mismatch')
    await protectTree(baseline, true)
    const run = Date.now().toString(36)
    const profiles = (sourceRun?.profiles ?? manifest.profiles).map(
      (_, i) => `conflict-e2e-${run}-${'abcde'[i]}`
    )
    const sourceHashes = []
    for (const [i, profile] of profiles.entries()) {
      const source = sourceRun ? profileDir(sourceRun.profiles[i]) : join(baseline, 'abcde'[i])
      const before = sourceRun ? await checksumTree(source) : null
      await copyUserdata(source, profileDir(profile))
      if (sourceRun) {
        if (JSON.stringify(before) !== JSON.stringify(await checksumTree(source)))
          throw new Error('Recovery source changed while cloning')
        sourceHashes.push(before)
      }
    }
    const runDir = join(root, `run-${run}`)
    await mkdir(runDir)
    await saveManifest(join(runDir, 'run.json'), {
      baseline: name,
      profiles,
      runDir,
      expected: sourceRun?.expected ?? manifest.expected,
      passphrase: sourceRun?.passphrase ?? manifest.passphrase,
      recoverySource: sourceRun ? relative(root, resolve(process.argv[4])) : undefined,
      sourceHashes: sourceRun ? sourceHashes : undefined,
    })
    await saveManifest(join(lock, 'owner.json'), { runDir })
    console.log(resolve(runDir, 'run.json'))
    // The lock stays held until the run is stopped; copied identities cannot run twice.
  } catch (error) {
    await rmdir(lock)
    throw error
  }
} else if (action === 'stop') {
  const path = process.argv[4]
  if (!path || !resolve(path).startsWith(root + '/run-'))
    throw new Error('Provide a run manifest under the test root')
  const run = JSON.parse(await readFile(path, 'utf8'))
  if (run.baseline !== name) throw new Error('Baseline mismatch')
  const owner = JSON.parse(await readFile(join(lock, 'owner.json'), 'utf8'))
  if (resolve(owner.runDir) !== resolve(run.runDir))
    throw new Error('This manifest does not own the active baseline lease')
  await stopRunProfiles(run.profiles)
  await unlink(join(lock, 'owner.json'))
  await rmdir(lock)
  console.log('Test daemons stopped; userdata and evidence retained')
} else
  throw new Error(
    'Usage: node e2e/conflict-userdata.mjs generate|restore|stop four|five [run.json]'
  )
