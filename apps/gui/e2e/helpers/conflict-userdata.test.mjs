import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'vitest'
import { validateTestProfile, copyUserdata } from './conflict-userdata.mjs'

test('only accepts dedicated conflict test profiles', () => {
  for (const profile of ['a', 'b', 'c', 'd', '../conflict-e2e-a', 'conflict-e2e-'])
    assert.throws(() => validateTestProfile(profile))
  assert.equal(validateTestProfile('conflict-e2e-12345-a'), 'conflict-e2e-12345-a')
})
test('copies persisted identity but excludes ephemeral process files', async () => {
  const root = await mkdtemp(join(tmpdir(), 'conflict-copy-test-'))
  const source = join(root, 'source'),
    dest = join(root, 'dest')
  await mkdir(join(source, 'vault'), { recursive: true })
  await writeFile(join(source, 'vault', 'test-key'), 'test-only')
  await writeFile(join(source, 'daemon.conn'), 'do-not-copy')
  await copyUserdata(source, dest)
  assert.equal(await readFile(join(dest, 'vault', 'test-key'), 'utf8'), 'test-only')
  await assert.rejects(readFile(join(dest, 'daemon.conn')))
  await assert.rejects(copyUserdata(source, dest))
})
test('refuses snapshots with uncheckpointed database writes', async () => {
  const root = await mkdtemp(join(tmpdir(), 'conflict-copy-wal-test-'))
  const source = join(root, 'source')
  await mkdir(source)
  await writeFile(join(source, 'control.sqlite-wal'), 'pending-test-write')
  await assert.rejects(copyUserdata(source, join(root, 'dest')), /Uncheckpointed database/)
})
