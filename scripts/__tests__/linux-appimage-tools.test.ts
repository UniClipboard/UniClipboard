import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  installPinnedLinuxdeploy,
  PINNED_LINUXDEPLOY,
  tauriToolsDirectory,
} from '../linux-appimage-tools.mjs'

const payload = Buffer.from('#!/bin/sh\necho linuxdeploy\n')
const pinned = {
  release: 'linuxdeploy-test',
  sha256: { x86_64: createHash('sha256').update(payload).digest('hex') },
}
const directories: string[] = []

function cacheEnv() {
  const cache = fs.mkdtempSync(path.join(os.tmpdir(), 'linuxdeploy-cache-'))
  directories.push(cache)
  return { XDG_CACHE_HOME: cache }
}

function respond(bytes: Buffer, ok = true) {
  return vi.fn(async () => ({
    ok,
    status: ok ? 200 : 404,
    arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length),
  }))
}

afterEach(() => {
  for (const directory of directories.splice(0)) fs.rmSync(directory, { recursive: true })
})

describe('Tauri tools directory', () => {
  it('follows the XDG cache directory like tauri-bundler', () => {
    expect(tauriToolsDirectory({ XDG_CACHE_HOME: '/cache', HOME: '/home/u' })).toBe('/cache/tauri')
  })

  it('falls back to ~/.cache', () => {
    expect(tauriToolsDirectory({ HOME: '/home/u' })).toBe('/home/u/.cache/tauri')
  })
})

describe('pinned linuxdeploy', () => {
  it('pins a checksum for every architecture the Linux bundles ship', () => {
    expect(Object.keys(PINNED_LINUXDEPLOY.sha256).sort()).toEqual(['aarch64', 'x86_64'])
    for (const digest of Object.values(PINNED_LINUXDEPLOY.sha256)) {
      expect(digest).toMatch(/^[0-9a-f]{64}$/)
    }
  })

  it('installs the verified tool under the name tauri-bundler looks up', async () => {
    const env = cacheEnv()
    const fetchImpl = respond(payload)

    const target = await installPinnedLinuxdeploy({ arch: 'x86_64', env, fetchImpl, pinned })

    expect(target).toBe(path.join(tauriToolsDirectory(env), 'linuxdeploy-x86_64.AppImage'))
    expect(fs.readFileSync(target)).toEqual(payload)
    expect(fs.statSync(target).mode & 0o111).not.toBe(0)
    expect(fetchImpl).toHaveBeenCalledWith(
      'https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-test/linuxdeploy-x86_64.AppImage'
    )
  })

  it('refuses a download whose checksum differs and installs nothing', async () => {
    const env = cacheEnv()

    await expect(
      installPinnedLinuxdeploy({
        arch: 'x86_64',
        env,
        fetchImpl: respond(Buffer.from('tampered')),
        pinned,
      })
    ).rejects.toThrow(/Unexpected linuxdeploy checksum/)

    expect(fs.existsSync(path.join(tauriToolsDirectory(env), 'linuxdeploy-x86_64.AppImage'))).toBe(
      false
    )
  })

  it('replaces a tool already in the cache, such as the unpinned legacy download', async () => {
    const env = cacheEnv()
    const target = path.join(tauriToolsDirectory(env), 'linuxdeploy-x86_64.AppImage')
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, 'legacy linuxdeploy')

    await installPinnedLinuxdeploy({ arch: 'x86_64', env, fetchImpl: respond(payload), pinned })

    expect(fs.readFileSync(target)).toEqual(payload)
  })

  it('restores a pristine copy on later runs without downloading again', async () => {
    const env = cacheEnv()
    const fetchImpl = respond(payload)
    const target = await installPinnedLinuxdeploy({ arch: 'x86_64', env, fetchImpl, pinned })
    fs.writeFileSync(target, 'patched in place by tauri-bundler')

    await installPinnedLinuxdeploy({ arch: 'x86_64', env, fetchImpl, pinned })

    expect(fs.readFileSync(target)).toEqual(payload)
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('reports a failed download', async () => {
    await expect(
      installPinnedLinuxdeploy({
        arch: 'x86_64',
        env: cacheEnv(),
        fetchImpl: respond(payload, false),
        pinned,
      })
    ).rejects.toThrow(/HTTP 404/)
  })

  it('rejects architectures without a pin', async () => {
    await expect(installPinnedLinuxdeploy({ arch: 'riscv64', env: cacheEnv() })).rejects.toThrow(
      /No pinned linuxdeploy/
    )
  })
})
