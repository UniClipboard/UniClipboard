import { describe, expect, it, vi } from 'vitest'
import { MAX_MERGE, buildCombinedNotes, mergeNotes, sortVersionsDesc } from '../merge'
import type { Manifest, ReleaseNotesArchive, VersionIndex } from '../types'

function archive(version: string, en = 'EN', zh = 'ZH'): ReleaseNotesArchive {
  return {
    version,
    channel: 'alpha',
    pub_date: '2026-01-01T00:00:00Z',
    notes_en: en,
    notes_zh: zh,
  }
}

function manifest(version: string, notes = 'latest notes'): Manifest {
  return {
    version,
    notes,
    pub_date: '2026-05-22T10:30:00Z',
    platforms: {
      'darwin-aarch64': { signature: 'sig', url: 'https://example.com/a' },
    },
  }
}

function indexDesc(...versions: string[]): VersionIndex {
  return {
    channel: 'alpha',
    updated_at: '2026-05-22T10:30:00Z',
    versions: versions.map(v => ({ version: v, pub_date: '2026-01-01T00:00:00Z' })),
  }
}

describe('sortVersionsDesc', () => {
  it('sorts semver correctly (10 > 9)', () => {
    const sorted = sortVersionsDesc([{ version: '0.9.0' }, { version: '0.10.0' }])
    expect(sorted.map(v => v.version)).toEqual(['0.10.0', '0.9.0'])
  })

  it('places stable above prerelease of same triplet', () => {
    const sorted = sortVersionsDesc([{ version: '0.10.0-alpha.5' }, { version: '0.10.0' }])
    expect(sorted.map(v => v.version)).toEqual(['0.10.0', '0.10.0-alpha.5'])
  })

  it('handles v-prefixed versions', () => {
    const sorted = sortVersionsDesc([{ version: 'v0.9.0' }, { version: 'v0.10.0' }])
    expect(sorted.map(v => v.version)).toEqual(['v0.10.0', 'v0.9.0'])
  })

  it('orders alpha.10 above alpha.2', () => {
    const sorted = sortVersionsDesc([{ version: '0.11.0-alpha.2' }, { version: '0.11.0-alpha.10' }])
    expect(sorted.map(v => v.version)).toEqual(['0.11.0-alpha.10', '0.11.0-alpha.2'])
  })
})

describe('buildCombinedNotes', () => {
  it('returns empty string for empty archives', () => {
    expect(
      buildCombinedNotes([], { truncated: false, omittedCount: 0, fromVersion: '0.10.0' })
    ).toBe('')
  })

  it('renders en + zh sections separated by <!-- zh -->', () => {
    const merged = buildCombinedNotes([archive('0.11.0', 'EN-NOTES', 'ZH-NOTES')], {
      truncated: false,
      omittedCount: 0,
      fromVersion: '0.10.0',
    })
    expect(merged).toContain('## v0.11.0')
    expect(merged).toContain('EN-NOTES')
    expect(merged).toContain('<!-- zh -->')
    expect(merged).toContain('ZH-NOTES')
    // English block precedes the Chinese block
    expect(merged.indexOf('EN-NOTES')).toBeLessThan(merged.indexOf('<!-- zh -->'))
    expect(merged.indexOf('<!-- zh -->')).toBeLessThan(merged.indexOf('ZH-NOTES'))
  })

  it('sorts multiple versions newest-first within each language block', () => {
    const merged = buildCombinedNotes(
      [archive('0.10.0', 'EN-OLD', 'ZH-OLD'), archive('0.11.0', 'EN-NEW', 'ZH-NEW')],
      { truncated: false, omittedCount: 0, fromVersion: '0.9.0' }
    )
    const enHeaders = [...merged.matchAll(/## v[0-9.]+/g)].map(m => m[0])
    expect(enHeaders.slice(0, 2)).toEqual(['## v0.11.0', '## v0.10.0'])
  })

  it('mentions truncation when truncated=true', () => {
    const merged = buildCombinedNotes([archive('0.11.0')], {
      truncated: true,
      omittedCount: 3,
      fromVersion: '0.5.0',
    })
    expect(merged).toMatch(/omitted/i)
    expect(merged).toContain('3')
    // Both en and zh preludes mention truncation
    const zhPart = merged.split('<!-- zh -->')[1]
    expect(zhPart).toMatch(/省略/)
  })
})

describe('mergeNotes', () => {
  it('returns latest manifest unchanged when from is not in index', async () => {
    const latest = manifest('0.11.0-alpha.6', 'LATEST-ONLY')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5')
    const fetcher = vi.fn()

    const result = await mergeNotes(latest, idx, '0.5.0-ancient', fetcher)

    expect(result.manifest).toEqual(latest)
    expect(result.manifest.notes).toBe('LATEST-ONLY')
    expect(result.mergedCount).toBe(1)
    expect(result.truncated).toBe(false)
    expect(fetcher).not.toHaveBeenCalled()
  })

  it('returns latest manifest unchanged when from equals latest', async () => {
    const latest = manifest('0.11.0-alpha.6', 'LATEST-ONLY')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5', '0.11.0-alpha.4')
    const fetcher = vi.fn()

    const result = await mergeNotes(latest, idx, '0.11.0-alpha.6', fetcher)

    expect(result.manifest.notes).toBe('LATEST-ONLY')
    expect(result.mergedCount).toBe(1)
    expect(fetcher).not.toHaveBeenCalled()
  })

  it('merges all intermediate versions when count <= MAX_MERGE', async () => {
    const latest = manifest('0.11.0-alpha.6')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5', '0.11.0-alpha.4', '0.11.0-alpha.3')
    const fetcher = vi.fn(async (version: string) =>
      archive(version, `EN-${version}`, `ZH-${version}`)
    )

    const result = await mergeNotes(latest, idx, '0.11.0-alpha.3', fetcher)

    expect(result.mergedCount).toBe(3) // a6, a5, a4 — excludes a3 (the from version)
    expect(result.truncated).toBe(false)
    expect(result.omittedCount).toBe(0)
    expect(fetcher).toHaveBeenCalledTimes(3)
    expect(result.manifest.notes).toContain('EN-0.11.0-alpha.6')
    expect(result.manifest.notes).toContain('EN-0.11.0-alpha.5')
    expect(result.manifest.notes).toContain('EN-0.11.0-alpha.4')
    expect(result.manifest.notes).not.toContain('EN-0.11.0-alpha.3') // from itself
  })

  it('truncates to MAX_MERGE versions and reports omittedCount', async () => {
    const latest = manifest('0.20.0')
    // 8 intermediate versions + 1 from = 9 total in index
    const idx = indexDesc(
      '0.20.0',
      '0.19.0',
      '0.18.0',
      '0.17.0',
      '0.16.0',
      '0.15.0',
      '0.14.0',
      '0.13.0',
      '0.10.0' // <- from
    )
    const fetcher = vi.fn(async (v: string) => archive(v))

    const result = await mergeNotes(latest, idx, '0.10.0', fetcher)

    expect(result.mergedCount).toBe(MAX_MERGE) // 5
    expect(result.truncated).toBe(true)
    expect(result.omittedCount).toBe(3) // 8 - 5
    expect(fetcher).toHaveBeenCalledTimes(MAX_MERGE)
    expect(result.manifest.notes).toMatch(/3.*omitted/i)
    // Should have selected the 5 newest of the 8 intermediates: 0.20, 0.19, 0.18, 0.17, 0.16
    expect(result.manifest.notes).toContain('## v0.16.0')
    expect(result.manifest.notes).not.toContain('## v0.15.0')
  })

  it('degrades to latest-only when all archive fetches return null', async () => {
    const latest = manifest('0.11.0-alpha.6', 'LATEST-ONLY')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5', '0.11.0-alpha.4')
    const fetcher = vi.fn(async () => null)

    const result = await mergeNotes(latest, idx, '0.11.0-alpha.4', fetcher)

    expect(result.manifest.notes).toBe('LATEST-ONLY')
    expect(result.mergedCount).toBe(1)
    expect(result.truncated).toBe(false)
  })

  it('skips null archives but keeps the rest', async () => {
    const latest = manifest('0.11.0-alpha.6')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5', '0.11.0-alpha.4')
    // a5 missing, a6 present
    const fetcher = vi.fn(async (v: string) => (v === '0.11.0-alpha.5' ? null : archive(v)))

    const result = await mergeNotes(latest, idx, '0.11.0-alpha.4', fetcher)

    expect(result.mergedCount).toBe(1)
    expect(result.manifest.notes).toContain('## v0.11.0-alpha.6')
    expect(result.manifest.notes).not.toContain('## v0.11.0-alpha.5')
  })

  it('handles v-prefixed fromVersion the same as bare', async () => {
    const latest = manifest('0.11.0-alpha.6')
    const idx = indexDesc('0.11.0-alpha.6', '0.11.0-alpha.5')
    const fetcher = vi.fn(async (v: string) => archive(v))

    const result = await mergeNotes(latest, idx, 'v0.11.0-alpha.5', fetcher)

    expect(result.mergedCount).toBe(1) // only a6 between (a5, latest]
  })
})
