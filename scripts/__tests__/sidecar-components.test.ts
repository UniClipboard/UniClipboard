import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { mergeSidecarComponents } from '../ci/merge-sidecar-components.mjs'

type ComponentRecord = {
  schema: number
  target: string
  buildMode: string
  sourceHead: string
  sourceDirty: boolean
  rustc: string
  cargoLockSha256: string
  run: { id: string; attempt: string; repository: string }
  files: Record<string, { sha256: string; bytes: number }>
}

const directories: string[] = []
function temporary() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-components-test-'))
  directories.push(directory)
  return directory
}
afterEach(() => {
  for (const directory of directories.splice(0))
    fs.rmSync(directory, { recursive: true, force: true })
})

describe('component staging execution', () => {
  function fixture() {
    const root = temporary()
    fs.mkdirSync(path.join(root, 'scripts'))
    fs.copyFileSync(
      path.resolve(__dirname, '../stage-daemon.mjs'),
      path.join(root, 'scripts/stage-daemon.mjs')
    )
    fs.mkdirSync(path.join(root, 'bin'))
    fs.writeFileSync(
      path.join(root, 'bin/cargo'),
      `#!/usr/bin/env node
const fs = require('node:fs'), path = require('node:path');
const args = process.argv.slice(2);
fs.appendFileSync('calls.jsonl', JSON.stringify(args) + '\\n');
if (process.env.TEST_BUILD_FAIL === '1') process.exit(42);
const targetIndex = args.indexOf('--target');
const target = targetIndex < 0 ? '' : args[targetIndex + 1];
const binary = args[args.indexOf('--bin') + 1];
const directory = path.join('target', target, args.includes('--release') ? 'release' : 'debug');
fs.mkdirSync(directory, { recursive: true });
fs.writeFileSync(path.join(directory, binary + (target.includes('windows') ? '.exe' : '')), binary);
`,
      { mode: 0o755 }
    )
    return root
  }
  function run(root: string, args: string[], extraEnv = {}) {
    return spawnSync(process.execPath, [path.join(root, 'scripts/stage-daemon.mjs'), ...args], {
      cwd: root,
      encoding: 'utf8',
      env: {
        ...process.env,
        UC_ENGINE_MBX_BIN: '',
        PATH: `${path.join(root, 'bin')}:${process.env.PATH}`,
        ...extraEnv,
      },
    })
  }
  function calls(root: string): string[][] {
    return fs
      .readFileSync(path.join(root, 'calls.jsonl'), 'utf8')
      .trim()
      .split('\n')
      .map(line => JSON.parse(line))
  }

  it.each([
    ['daemon', 'uc-daemon', 'uniclipd'],
    ['helper', 'quick-panel', 'uniclip-quick-panel'],
  ])(
    'builds and stages only %s without selecting the other Cargo package',
    (component, pkg, binary) => {
      const root = fixture()
      const target = 'aarch64-pc-windows-msvc'
      const result = run(root, ['--target', target, '--component', component, '--timings'])
      expect(result.status, result.stderr).toBe(0)
      expect(calls(root)).toEqual([
        [
          'build',
          '--locked',
          '-p',
          pkg,
          '--bin',
          binary,
          '--release',
          '--target',
          target,
          '--timings',
        ],
      ])
      expect(fs.readdirSync(path.join(root, 'target/sidecar-staging'))).toEqual([
        `${binary}-${target}.exe`,
      ])
      expect(
        fs.readFileSync(path.join(root, `target/sidecar-staging/${binary}-${target}.exe`), 'utf8')
      ).toBe(binary)
    }
  )

  it('keeps full local staging and executable permissions on macOS', () => {
    const root = fixture()
    const target = 'x86_64-apple-darwin'
    const result = run(root, ['--target', target, '--debug'])
    expect(result.status, result.stderr).toBe(0)
    expect(calls(root).map(args => args[args.indexOf('-p') + 1])).toEqual([
      'uc-daemon',
      'quick-panel',
    ])
    for (const binary of ['uniclipd', 'uniclip-quick-panel']) {
      expect(
        fs.statSync(path.join(root, `target/sidecar-staging/${binary}-${target}`)).mode & 0o111
      ).toBe(0o111)
    }
  })

  it('rejects a helper on Linux before invoking Cargo', () => {
    const root = fixture()
    const result = run(root, ['--target', 'aarch64-unknown-linux-gnu', '--component', 'helper'])
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain('not supported')
    expect(fs.existsSync(path.join(root, 'calls.jsonl'))).toBe(false)
  })

  it('does not stage a failed build or continue to the next component', () => {
    const root = fixture()
    const result = run(root, ['--target', 'aarch64-apple-darwin'], { TEST_BUILD_FAIL: '1' })
    expect(result.status).not.toBe(0)
    expect(calls(root)).toHaveLength(1)
    expect(fs.readdirSync(path.join(root, 'target/sidecar-staging'))).toEqual([])
  })
})

describe('component build evidence assembly', () => {
  function fixture(target = 'aarch64-pc-windows-msvc', buildMode = 'release') {
    const root = temporary()
    const sourceSha = 'a'.repeat(40)
    const options = {
      target,
      buildMode,
      sourceSha,
      artifacts: path.join(root, 'artifacts'),
      out: path.join(root, 'out'),
      runId: '123',
      runAttempt: '2',
    }
    function archive(
      component: string,
      change: (record: ComponentRecord, directory: string, binary: string) => void = () => {}
    ) {
      const directory = path.join(root, component)
      fs.mkdirSync(directory, { recursive: true })
      const binary = `${component === 'daemon' ? 'uniclipd' : 'uniclip-quick-panel'}-${target}${target.includes('windows') ? '.exe' : ''}`
      const contents = Buffer.from(`${component} contents`)
      fs.writeFileSync(path.join(directory, binary), contents, { mode: 0o755 })
      const record = {
        schema: 1,
        target,
        buildMode,
        sourceHead: sourceSha,
        sourceDirty: false,
        rustc: 'rustc 1.95.0',
        cargoLockSha256: 'lock-hash',
        run: { id: '123', attempt: '2', repository: 'UniClipboard/UniClipboard' },
        files: {
          [binary]: {
            sha256: createHash('sha256').update(contents).digest('hex'),
            bytes: contents.length,
          },
        },
      }
      change(record, directory, binary)
      fs.writeFileSync(path.join(directory, 'sidecar-provenance.json'), JSON.stringify(record))
      const destination = path.join(
        options.artifacts,
        `sidecar-component-${target}${buildMode === 'test' ? '-test' : ''}-${component}`
      )
      fs.mkdirSync(destination, { recursive: true })
      execFileSync('tar', ['-cf', path.join(destination, 'sidecar.tar'), '-C', directory, '.'])
      return { binary, record }
    }
    return { options, archive }
  }

  it.each(['release', 'test'])(
    'merges %s evidence without changing build identity or hashes',
    buildMode => {
      const { options, archive } = fixture('aarch64-apple-darwin', buildMode)
      const daemon = archive('daemon'),
        helper = archive('helper')
      mergeSidecarComponents(options)
      const record = JSON.parse(
        fs.readFileSync(path.join(options.out, 'sidecar-provenance.json'), 'utf8')
      )
      expect(record).toEqual({
        ...daemon.record,
        files: { ...daemon.record.files, ...helper.record.files },
      })
      for (const { binary } of [daemon, helper]) {
        const contents = fs.readFileSync(path.join(options.out, binary))
        expect(createHash('sha256').update(contents).digest('hex')).toBe(
          record.files[binary].sha256
        )
        expect(fs.statSync(path.join(options.out, binary)).mode & 0o111).toBe(0o111)
      }
    }
  )

  it('accepts the flat daemon-only Linux download produced by download-artifact v6', () => {
    const { options, archive } = fixture('aarch64-unknown-linux-gnu')
    const { binary } = archive('daemon')
    fs.renameSync(
      path.join(options.artifacts, `sidecar-component-${options.target}-daemon`, 'sidecar.tar'),
      path.join(options.artifacts, 'sidecar.tar')
    )
    mergeSidecarComponents(options)
    expect(fs.readdirSync(options.out).sort()).toEqual([binary, 'sidecar-provenance.json'].sort())
  })

  it.each([
    [
      'another source',
      (r: ComponentRecord) => {
        r.sourceHead = 'b'.repeat(40)
      },
    ],
    [
      'dirty source',
      (r: ComponentRecord) => {
        r.sourceDirty = true
      },
    ],
    [
      'another target',
      (r: ComponentRecord) => {
        r.target = 'x86_64-pc-windows-msvc'
      },
    ],
    [
      'another mode',
      (r: ComponentRecord) => {
        r.buildMode = 'test'
      },
    ],
    [
      'another attempt',
      (r: ComponentRecord) => {
        r.run.attempt = '1'
      },
    ],
    [
      'another run',
      (r: ComponentRecord) => {
        r.run.id = '456'
      },
    ],
    [
      'another compiler',
      (r: ComponentRecord) => {
        r.rustc = 'rustc different'
      },
    ],
    [
      'another lockfile',
      (r: ComponentRecord) => {
        r.cargoLockSha256 = 'different'
      },
    ],
    [
      'changed content',
      (_r: ComponentRecord, dir: string, binary: string) => {
        fs.writeFileSync(path.join(dir, binary), 'tampered')
      },
    ],
    [
      'unrecorded files',
      (_r: ComponentRecord, dir: string) => {
        fs.writeFileSync(path.join(dir, 'extra'), 'extra')
      },
    ],
    [
      'a symbolic link',
      (_r: ComponentRecord, dir: string, binary: string) => {
        fs.unlinkSync(path.join(dir, binary))
        fs.symlinkSync('/dev/null', path.join(dir, binary))
      },
    ],
  ])('refuses %s before writing any merged output', (_name, change) => {
    const { options, archive } = fixture()
    archive('daemon')
    archive('helper', change)
    expect(() => mergeSidecarComponents(options)).toThrow()
    expect(fs.existsSync(options.out)).toBe(false)
  })
})
