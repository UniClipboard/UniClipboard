import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'
import { selectBuildPlatforms } from '../ci/select-build-platforms.mjs'

const read = (file: string) =>
  parse(fs.readFileSync(path.resolve(__dirname, '../../.github/workflows', file), 'utf8'))

describe('independent release platform pipelines', () => {
  it('splits every macOS and Windows architecture before compiling or packaging', () => {
    expect(selectBuildPlatforms('all')).toEqual([
      'macos-aarch64',
      'macos-x86_64',
      'windows-x86_64',
      'windows-arm64',
      'linux-gui',
    ])
    expect(selectBuildPlatforms('windows-latest')).toEqual(['windows-x86_64', 'windows-arm64'])
    for (const platform of selectBuildPlatforms('all')) {
      expect(selectBuildPlatforms(platform)).toEqual([platform])
    }
  })

  it('preserves the Linux GUI exclusion in all/test and build-only runs', () => {
    expect(selectBuildPlatforms('all', { buildMode: 'test' })).not.toContain('linux-gui')
    expect(selectBuildPlatforms('all', { buildOnly: true })).not.toContain('linux-gui')
    expect(selectBuildPlatforms('linux-gui', { buildMode: 'test' })).toEqual(['linux-gui'])
    expect(selectBuildPlatforms('ubuntu-22.04')).toEqual(['ubuntu-22.04'])
    expect(selectBuildPlatforms('ubuntu-22.04-arm')).toEqual(['ubuntu-22.04-arm'])
    expect(() => selectBuildPlatforms('unsupported')).toThrow('Unknown build platform')
  })

  it('pins a moving source ref once and forwards signing/acceptance controls to each pipeline', () => {
    const outer = read('build.yml')
    const inner = read('build-target.yml')
    const call = outer.jobs['build-platform']
    expect(call.uses).toBe('./.github/workflows/build-target.yml')
    const setup = outer.jobs['setup-platforms'].steps
    expect(setup[0].with.ref).toBe('${{ github.sha }}')
    expect(setup.find((s: { id?: string }) => s.id === 'source').run).toContain(
      'git rev-parse FETCH_HEAD'
    )
    expect(call.with.branch).toBe('${{ needs.setup-platforms.outputs.source_sha }}')
    expect(call.with.platform).toBe('${{ matrix.platform }}')
    expect(call.strategy['fail-fast']).toBe("${{ github.workflow == 'Release' }}")
    expect(call.secrets).toBe('inherit')
    for (const name of Object.keys(inner.on.workflow_call.inputs)) {
      expect(call.with, name).toHaveProperty(name)
    }
    for (const name of [
      'require_signing',
      'signing_self_test',
      'signpath_test_signing',
      'allow_unsigned_windows',
    ]) {
      expect(call.with[name]).toBe(`\${{ inputs.${name} == true }}`)
    }
  })

  it('builds daemon and helper independently, then verifies both before packaging', () => {
    const jobs = read('build-target.yml').jobs
    expect(jobs['setup-matrix']).toBeUndefined()
    expect(jobs['build-component'].needs).toBeUndefined()
    expect(read('build.yml').jobs['build-platform'].with.plan).toBe('${{ matrix.plan }}')
    expect(jobs['build-sidecar'].needs).toBe('build-component')
    expect(
      jobs['build-sidecar'].steps.some((step: { run?: string }) =>
        step.run?.includes('merge-sidecar-components.mjs')
      )
    ).toBe(true)
    for (const consumer of ['package-macos-gui', 'package-windows-gui', 'build-cli']) {
      expect(jobs[consumer].needs).toBe('build-sidecar')
    }
  })

  it('keeps release cache writes disabled while warmup explicitly populates downloads', () => {
    const release = read('release.yml')
    expect(release.jobs.build.strategy['fail-fast']).toBe(true)
    expect(release.jobs.build.strategy.matrix.pipeline).toEqual(['desktop', 'cli'])
    expect(release.jobs.build.uses).toBe('./.github/workflows/release-build.yml')
    expect(release.jobs['create-release'].needs).toEqual(['validate', 'build'])
    const pipelines = read('release-build.yml').jobs
    expect(pipelines.desktop.with.save_cache).toBe(false)
    expect(read('release.yml').jobs.build.secrets).toBe('inherit')
    expect(pipelines.cli.with.save_cache).toBe(false)
    const linux = read('package-linux-gui.yml').jobs.package.steps
    expect(
      linux.find((step: { name?: string }) => step.name === 'Restore Cargo downloads').uses
    ).toBe('actions/cache/restore@v4')
    expect(
      linux.find((step: { name?: string }) => step.name === 'Save the Rust cache').if
    ).toContain('inputs.save_cache != false')
    expect(read('build-target.yml').jobs['package-linux-gui'].with.save_cache).toBe(
      '${{ inputs.save_cache == true }}'
    )
    const warmup = read('cache-warmup.yml').jobs
    expect(warmup['warmup-release-app'].with.save_cache).toBe(true)
    expect(warmup['warmup-test-app'].with.save_cache).toBe(true)
    const cache = read('build-target.yml').jobs['build-component'].steps.find(
      (step: { uses?: string }) => step.uses === 'Swatinem/rust-cache@v2'
    )
    expect(cache.with['cache-targets']).toBe(false)
    expect(cache.with['save-if']).toContain("github.event_name == 'push'")
  })
})

// Every nested matrix must propagate failures before its remaining targets finish.
describe('release failure propagation', () => {
  it.each(['build.yml', 'build-target.yml', 'build-cli.yml', 'package-linux-gui.yml'])(
    'enables fail-fast only for Release in %s',
    file => {
      for (const job of Object.values(read(file).jobs) as {
        strategy?: Record<string, unknown>
      }[]) {
        if (job.strategy) {
          expect(job.strategy['fail-fast']).toBe("${{ github.workflow == 'Release' }}")
        }
      }
    }
  )
})
