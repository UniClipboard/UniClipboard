import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { globSync } from 'glob'
import { describe, expect, it } from 'vitest'

const root = resolve(import.meta.dirname, '../../../..')

describe('Base UI migration', () => {
  it('uses Base UI as the only shared component primitive library', () => {
    const packageJson = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8')) as {
      dependencies?: Record<string, string>
    }
    const componentsJson = JSON.parse(readFileSync(resolve(root, 'components.json'), 'utf8')) as {
      style?: string
    }
    const frontendSources = globSync(resolve(root, 'src/**/*.{ts,tsx}'))
      .map(path => readFileSync(path, 'utf8'))
      .join('\n')
    const sliderSource = readFileSync(resolve(root, 'src/components/ui/slider.tsx'), 'utf8')

    expect(componentsJson.style).toBe('base-nova')
    expect(packageJson.dependencies?.['@base-ui/react']).toBeDefined()
    expect(Object.keys(packageJson.dependencies ?? {})).not.toContain('radix-ui')
    expect(
      Object.keys(packageJson.dependencies ?? {}).some(name => name.startsWith('@radix-ui/'))
    ).toBe(false)
    expect(frontendSources).not.toMatch(
      /(?:from\s+|import\()['"](?:@radix-ui\/|radix-ui(?:\/internal)?['"])/
    )
    expect(sliderSource).not.toMatch(/key=\{`[^`]*\$\{index\}/)
  })
})
