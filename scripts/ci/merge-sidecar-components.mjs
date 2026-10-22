import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  copyFileSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { isDeepStrictEqual } from 'node:util'

// Retain the build jobs' provenance rather than generating a new claim on an assembly runner.
export function mergeSidecarComponents({
  target,
  buildMode,
  sourceSha,
  artifacts,
  out,
  runId,
  runAttempt,
}) {
  if (!/^[a-z0-9_]+(?:-[a-z0-9_]+)+$/.test(target)) throw new Error('Invalid target triple')
  if (!['release', 'test'].includes(buildMode)) throw new Error('Invalid build mode')
  const suffix = buildMode === 'test' ? '-test' : ''
  const exe = target.includes('windows') ? '.exe' : ''
  const components = [{ component: 'daemon', binary: `uniclipd-${target}${exe}` }]
  if (target.includes('apple-darwin') || target.includes('windows')) {
    components.push({ component: 'helper', binary: `uniclip-quick-panel-${target}${exe}` })
  }
  const temporary = mkdtempSync(join(tmpdir(), 'uc-sidecar-merge-'))
  try {
    let merged
    const verified = []
    for (const { component, binary } of components) {
      const archive = resolve(
        artifacts,
        `sidecar-component-${target}${suffix}-${component}`,
        'sidecar.tar'
      )
      // Only regular files with the exact names emitted by the component builder may be extracted.
      const listing = execFileSync('tar', ['-tf', archive], { encoding: 'utf8' })
        .trim()
        .split('\n')
        .map(name => name.replace(/^\.\//, ''))
        .filter(Boolean)
      const expected = [binary, 'sidecar-provenance.json'].sort()
      if (!isDeepStrictEqual(listing.sort(), expected))
        throw new Error(`Unexpected archive entries for ${component}`)
      const directory = join(temporary, component)
      mkdirSync(directory)
      execFileSync('tar', ['-xf', archive, '-C', directory])
      for (const name of expected) {
        if (!lstatSync(join(directory, name)).isFile())
          throw new Error(`Not a regular file: ${name}`)
      }
      const record = JSON.parse(readFileSync(join(directory, 'sidecar-provenance.json'), 'utf8'))
      if (
        record.schema !== 1 ||
        record.target !== target ||
        record.buildMode !== buildMode ||
        record.sourceHead !== sourceSha ||
        record.sourceDirty !== false
      ) {
        throw new Error(`Component provenance does not match the pinned source: ${component}`)
      }
      if (
        (runId && record.run?.id !== runId) ||
        (runAttempt && record.run?.attempt !== runAttempt)
      ) {
        throw new Error(`Component provenance belongs to another workflow attempt: ${component}`)
      }
      const { files, ...identity } = record
      if (!isDeepStrictEqual(Object.keys(files).sort(), [binary]))
        throw new Error(`Unexpected component files: ${component}`)
      const contents = readFileSync(join(directory, binary))
      if (
        files[binary].sha256 !== createHash('sha256').update(contents).digest('hex') ||
        files[binary].bytes !== contents.length
      ) {
        throw new Error(`Component binary does not match its build provenance: ${component}`)
      }
      if (merged) {
        const { files: _files, ...previousIdentity } = merged
        if (!isDeepStrictEqual(identity, previousIdentity))
          throw new Error('Component build identities differ')
        Object.assign(merged.files, files)
      } else {
        merged = record
      }
      verified.push({ directory, binary })
    }
    mkdirSync(out, { recursive: true })
    for (const { directory, binary } of verified) {
      copyFileSync(join(directory, binary), join(out, binary))
    }
    writeFileSync(join(out, 'sidecar-provenance.json'), JSON.stringify(merged, null, 2) + '\n')
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const arg = name => {
    const index = process.argv.indexOf(name)
    if (index < 0 || !process.argv[index + 1]) throw new Error(`${name} is required`)
    return process.argv[index + 1]
  }
  mergeSidecarComponents({
    target: arg('--target'),
    buildMode: arg('--build-mode'),
    sourceSha: arg('--source-sha'),
    artifacts: arg('--artifacts'),
    out: arg('--out'),
    runId: process.env.GITHUB_RUN_ID,
    runAttempt: process.env.GITHUB_RUN_ATTEMPT,
  })
}
