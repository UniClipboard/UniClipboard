import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import {
  HOSTED_WINDOWS_LABEL,
  resolveWindowsRunner,
  SELF_HOSTED_WINDOWS_LABEL,
  selectWindowsRunner,
} from '../ci/select-windows-runner.mjs'

const runner = (status: string, busy: boolean, labels = [SELF_HOSTED_WINDOWS_LABEL]) => ({
  status,
  busy,
  labels: labels.map(name => ({ name })),
})

describe('Windows Actions runner selection', () => {
  it('uses the dedicated self-hosted runner only when it is online and idle', () => {
    expect(selectWindowsRunner([runner('online', false)])).toEqual({
      runner: SELF_HOSTED_WINDOWS_LABEL,
      source: 'self-hosted',
    })
  })

  it.each([
    { runners: [runner('offline', false)] },
    { runners: [runner('online', true)] },
    { runners: [runner('online', false, ['another-repository'])] },
    { runners: [] },
  ])('uses GitHub-hosted Windows when the dedicated runner is unavailable', ({ runners }) => {
    expect(selectWindowsRunner(runners)).toEqual({
      runner: HOSTED_WINDOWS_LABEL,
      source: 'github-hosted',
    })
  })

  it('falls back when the status API fails', async () => {
    const fetchImpl = vi.fn().mockRejectedValue(new Error('network unavailable'))
    await expect(
      resolveWindowsRunner({
        apiUrl: 'https://api.github.test',
        repository: 'UniClipboard/UniClipboard',
        token: 'test-token',
        fetchImpl,
      })
    ).resolves.toMatchObject({
      runner: HOSTED_WINDOWS_LABEL,
      source: 'github-hosted',
      reason: 'api-error',
    })
  })

  it('falls back when no status token is configured', async () => {
    await expect(
      resolveWindowsRunner({
        apiUrl: 'https://api.github.test',
        repository: 'UniClipboard/UniClipboard',
        token: '',
      })
    ).resolves.toEqual({
      runner: HOSTED_WINDOWS_LABEL,
      source: 'github-hosted',
      reason: 'missing-token',
    })
  })

  it('uses the dedicated status secret without exposing it to build jobs', () => {
    for (const file of ['build.yml', 'build-cli.yml']) {
      const source = fs.readFileSync(
        path.resolve(__dirname, '../../.github/workflows', file),
        'utf8'
      )
      expect(source).toContain('RUNNER_STATUS_TOKEN: ${{ secrets.WINDOWS_RUNNER_STATUS_TOKEN }}')
      expect(source.match(/WINDOWS_RUNNER_STATUS_TOKEN/g)).toHaveLength(1)
    }
  })
})
