import { describe, expect, it } from 'vitest'
import { buildTargetPlan } from '../ci/build-target-plan.mjs'
import { selectBuildPlatforms } from '../ci/select-build-platforms.mjs'

describe('shared build target plan', () => {
  it('keeps component features and native acceptance architectures separate', () => {
    for (const platform of [...selectBuildPlatforms('all'), 'ubuntu-22.04', 'ubuntu-22.04-arm']) {
      const plan = buildTargetPlan(platform)
      expect(plan.matrix).toHaveLength(platform === 'linux-gui' ? 0 : 1)
      expect(plan.components.map(e => e.component)).toEqual(
        platform === 'linux-gui'
          ? []
          : platform.startsWith('ubuntu')
            ? ['daemon']
            : ['daemon', 'helper']
      )
      for (const entry of plan.components) expect(entry.target).toBe(plan.matrix[0].target)
      if (platform.startsWith('windows')) {
        expect(plan.components[0].runner).toBe('windows-latest')
        expect(plan.windows[0].smoke).toBe(
          platform.endsWith('arm64') ? 'windows-11-arm' : 'windows-latest'
        )
      }
      if (platform === 'macos-x86_64') expect(plan.macosSmoke[0].runner).toBe('macos-15-intel')
    }
  })
  it('leaves Windows CLI signing under SignPath ownership', () => {
    expect(buildTargetPlan('windows-x86_64').cli).toHaveLength(1)
    expect(buildTargetPlan('windows-x86_64', { signpathProduction: true }).cli).toEqual([])
    expect(buildTargetPlan('windows-arm64').cli).toEqual([])
    expect(() => buildTargetPlan('unknown')).toThrow('Unknown build platform')
  })
})
