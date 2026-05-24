import { describe, expect, it } from 'vitest'
import { buildArchive, upsertVersionIntoIndex } from '../archive-release-notes.js'

interface VersionEntry {
  version: string
  pub_date: string
}

interface VersionIndex {
  channel: string
  updated_at: string
  versions: VersionEntry[]
}

function emptyIndex(channel = 'alpha'): VersionIndex {
  return { channel, updated_at: '1970-01-01T00:00:00Z', versions: [] }
}

describe('upsertVersionIntoIndex', () => {
  it('adds the first version', () => {
    const result: VersionIndex = upsertVersionIntoIndex(emptyIndex(), {
      version: '0.11.0-alpha.6',
      pub_date: '2026-05-22T10:30:00Z',
    })

    expect(result.versions).toHaveLength(1)
    expect(result.versions[0].version).toBe('0.11.0-alpha.6')
    expect(result.channel).toBe('alpha')
  })

  it('sorts versions descending by semver', () => {
    const seed: VersionIndex = {
      channel: 'alpha',
      updated_at: '2026-01-01T00:00:00Z',
      versions: [
        { version: '0.10.0', pub_date: '2026-04-01T00:00:00Z' },
        { version: '0.9.0', pub_date: '2026-03-01T00:00:00Z' },
      ],
    }

    const result: VersionIndex = upsertVersionIntoIndex(seed, {
      version: '0.11.0-alpha.6',
      pub_date: '2026-05-22T10:30:00Z',
    })

    expect(result.versions.map(v => v.version)).toEqual(['0.11.0-alpha.6', '0.10.0', '0.9.0'])
  })

  it('handles 0.10.0 vs 0.9.0 correctly (not string-sorted)', () => {
    const result: VersionIndex = upsertVersionIntoIndex(
      {
        channel: 'stable',
        updated_at: '2026-01-01T00:00:00Z',
        versions: [{ version: '0.9.0', pub_date: '2026-03-01T00:00:00Z' }],
      },
      { version: '0.10.0', pub_date: '2026-04-01T00:00:00Z' }
    )

    expect(result.versions[0].version).toBe('0.10.0')
    expect(result.versions[1].version).toBe('0.9.0')
  })

  it('places alpha prerelease below stable of same triplet', () => {
    const result: VersionIndex = upsertVersionIntoIndex(
      {
        channel: 'alpha',
        updated_at: '2026-01-01T00:00:00Z',
        versions: [{ version: '0.10.0-alpha.5', pub_date: '2026-04-01T00:00:00Z' }],
      },
      { version: '0.10.0', pub_date: '2026-04-15T00:00:00Z' }
    )

    expect(result.versions.map(v => v.version)).toEqual(['0.10.0', '0.10.0-alpha.5'])
  })

  it('orders alpha.10 above alpha.2', () => {
    const result: VersionIndex = upsertVersionIntoIndex(
      {
        channel: 'alpha',
        updated_at: '2026-01-01T00:00:00Z',
        versions: [{ version: '0.11.0-alpha.2', pub_date: '2026-01-01T00:00:00Z' }],
      },
      { version: '0.11.0-alpha.10', pub_date: '2026-05-01T00:00:00Z' }
    )

    expect(result.versions[0].version).toBe('0.11.0-alpha.10')
    expect(result.versions[1].version).toBe('0.11.0-alpha.2')
  })

  it('deduplicates the same version (new entry replaces old)', () => {
    const result: VersionIndex = upsertVersionIntoIndex(
      {
        channel: 'alpha',
        updated_at: '2026-01-01T00:00:00Z',
        versions: [{ version: '0.11.0-alpha.6', pub_date: '2026-05-20T00:00:00Z' }],
      },
      { version: '0.11.0-alpha.6', pub_date: '2026-05-22T10:30:00Z' }
    )

    expect(result.versions).toHaveLength(1)
    expect(result.versions[0].pub_date).toBe('2026-05-22T10:30:00Z')
  })

  it('strips v-prefix via normalization but preserves original format in entry', () => {
    // 调用方应传入规范化的版本字符串；这里的 normalizer 仅用于排序，不会改写条目本身。
    const result: VersionIndex = upsertVersionIntoIndex(emptyIndex(), {
      version: 'v0.11.0',
      pub_date: '2026-05-22T10:30:00Z',
    })
    expect(result.versions[0].version).toBe('v0.11.0')
  })

  it('updates updated_at timestamp', () => {
    const original = emptyIndex()
    const before = original.updated_at
    const result: VersionIndex = upsertVersionIntoIndex(original, {
      version: '0.11.0',
      pub_date: '2026-05-22T10:30:00Z',
    })
    expect(result.updated_at).not.toBe(before)
    expect(result.updated_at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/)
  })
})

describe('buildArchive', () => {
  it('packages all fields with snake_case keys', () => {
    const archive = buildArchive({
      version: '0.11.0-alpha.6',
      channel: 'alpha',
      pubDate: '2026-05-22T10:30:00Z',
      notesEn: 'EN body',
      notesZh: 'ZH body',
    })

    expect(archive).toEqual({
      version: '0.11.0-alpha.6',
      channel: 'alpha',
      pub_date: '2026-05-22T10:30:00Z',
      notes_en: 'EN body',
      notes_zh: 'ZH body',
    })
  })
})
