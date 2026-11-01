import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'

export const buildLeaseEnv = 'UC_GUI_E2E_BUILD_LEASE'
const kind = 'gui-e2e-build'
const read = async filename => JSON.parse(await readFile(filename, 'utf8'))

export async function allocateBuildLease() {
  const directory = await mkdtemp(path.join(tmpdir(), 'uc-gui-browser-'))
  await writeFile(
    path.join(directory, 'lease.json'),
    JSON.stringify({ kind, ownerPid: process.pid }),
    { mode: 0o600 }
  )
  return directory
}

export async function claimBuildLease(directory) {
  const lease = await read(path.join(directory, 'lease.json'))
  if (lease.kind !== kind || !Number.isSafeInteger(lease.ownerPid) || lease.ownerPid <= 0)
    throw new Error('Invalid GUI build lease')
  process.kill(lease.ownerPid, 0)
  await writeFile(path.join(directory, 'server.json'), JSON.stringify({ kind, pid: process.pid }), {
    mode: 0o600,
  })
  return path.join(directory, 'site')
}

export async function releaseBuildLease(directory) {
  const receipt = { leaseId: path.basename(directory), removed: false, passed: false }
  try {
    const lease = await read(path.join(directory, 'lease.json'))
    if (lease.kind !== kind || lease.ownerPid !== process.pid)
      throw new Error('Unowned GUI build lease')
    let server
    try {
      server = await read(path.join(directory, 'server.json'))
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
    if (server) {
      if (server.kind !== kind || !Number.isSafeInteger(server.pid) || server.pid <= 0)
        throw new Error('Unknown GUI fixture identity')
      receipt.serverPid = server.pid
      try {
        process.kill(server.pid, 0)
        return { ...receipt, preserved: true, reason: 'service-pid-still-present' }
      } catch (error) {
        if (error.code !== 'ESRCH') throw error
      }
    }
    await rm(directory, { recursive: true })
    return { ...receipt, removed: true, passed: true, servicePidAbsent: server ? true : null }
  } catch (error) {
    return { ...receipt, preserved: true, errorCode: error.code || error.name }
  }
}
