import { describe, expect, it } from 'vitest'
import {
  fetchPublishedReleases,
  selectPreviousPublishedRelease,
  stripVersionTagPrefix,
} from '../previous-published-release.js'

describe('stripVersionTagPrefix', () => {
  it('removes the leading v from version tags', () => {
    expect(stripVersionTagPrefix('v0.4.0-alpha.4')).toBe('0.4.0-alpha.4')
    expect(stripVersionTagPrefix('0.4.0-alpha.4')).toBe('0.4.0-alpha.4')
  })
})

describe('selectPreviousPublishedRelease', () => {
  it('uses the latest published prerelease below the target version', () => {
    const release = selectPreviousPublishedRelease(
      [
        {
          tagName: 'v0.4.0-alpha.4',
          isDraft: false,
          isPrerelease: true,
          publishedAt: '2026-04-06T11:55:23Z',
        },
        {
          tagName: 'v0.4.0-alpha.3',
          isDraft: false,
          isPrerelease: true,
          publishedAt: '2026-04-06T08:25:34Z',
        },
        {
          tagName: 'v0.3.3',
          isDraft: false,
          isPrerelease: false,
          publishedAt: '2026-03-19T14:14:16Z',
        },
      ],
      '0.4.0-alpha.6'
    )

    expect(release?.tagName).toBe('v0.4.0-alpha.4')
    expect(release?.version).toBe('0.4.0-alpha.4')
  })

  it('ignores draft releases when finding the previous published version', () => {
    const release = selectPreviousPublishedRelease(
      [
        {
          tagName: 'v0.4.0-alpha.5',
          isDraft: true,
          isPrerelease: true,
          publishedAt: null,
        },
        {
          tagName: 'v0.4.0-alpha.4',
          isDraft: false,
          isPrerelease: true,
          publishedAt: '2026-04-06T11:55:23Z',
        },
      ],
      '0.4.0-alpha.6'
    )

    expect(release?.tagName).toBe('v0.4.0-alpha.4')
  })

  it('falls back to the latest published stable release when no earlier prerelease exists', () => {
    const release = selectPreviousPublishedRelease(
      [
        {
          tagName: 'v0.3.3',
          isDraft: false,
          isPrerelease: false,
          publishedAt: '2026-03-19T14:14:16Z',
        },
        {
          tagName: 'v0.3.2',
          isDraft: false,
          isPrerelease: false,
          publishedAt: '2026-03-18T08:58:21Z',
        },
      ],
      '0.4.0-alpha.1'
    )

    expect(release?.tagName).toBe('v0.3.3')
  })

  it('uses the latest stable release before the target when preparing a stable release', () => {
    const release = selectPreviousPublishedRelease(
      [
        {
          tagName: 'v0.8.0-alpha.3',
          isDraft: false,
          isPrerelease: true,
          publishedAt: '2026-05-10T10:15:00Z',
        },
        {
          tagName: 'v0.7.2',
          isDraft: false,
          isPrerelease: false,
          publishedAt: '2026-04-30T08:20:00Z',
        },
        {
          tagName: 'v0.7.1',
          isDraft: false,
          isPrerelease: false,
          publishedAt: '2026-04-20T08:20:00Z',
        },
      ],
      '0.8.0'
    )

    expect(release?.tagName).toBe('v0.7.2')
    expect(release?.version).toBe('0.7.2')
  })

  it('returns null when no published release exists below the target version', () => {
    const release = selectPreviousPublishedRelease(
      [
        {
          tagName: 'v0.4.0-alpha.1',
          isDraft: false,
          isPrerelease: true,
          publishedAt: '2026-03-17T16:28:20Z',
        },
      ],
      '0.4.0-alpha.1'
    )

    expect(release).toBeNull()
  })
})

describe('fetchPublishedReleases', () => {
  it('normalizes GitHub API release fields before selection', async () => {
    const releases = await fetchPublishedReleases(
      'UniClipboard/UniClipboard',
      undefined,
      async () =>
        new Response(
          JSON.stringify([
            {
              tag_name: 'v0.4.0-alpha.4',
              draft: false,
              prerelease: true,
              published_at: '2026-04-06T11:55:23Z',
            },
          ]),
          {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }
        )
    )

    expect(releases).toEqual([
      {
        tagName: 'v0.4.0-alpha.4',
        isDraft: false,
        isPrerelease: true,
        publishedAt: '2026-04-06T11:55:23Z',
        version: '0.4.0-alpha.4',
      },
    ])
  })
})
