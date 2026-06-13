import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

function readWorkflow(name: string) {
  return fs.readFileSync(path.resolve(__dirname, '../../.github/workflows', name), 'utf8')
}

describe('npm publish workflow trigger', () => {
  it('uses repository_dispatch for alpha npm publish handoff', () => {
    const releaseWorkflow = readWorkflow('release.yml')
    const npmDispatchJob = releaseWorkflow.slice(releaseWorkflow.indexOf('  dispatch-npm-alpha:'))

    expect(npmDispatchJob).toContain('"repos/${{ github.repository }}/dispatches"')
    expect(npmDispatchJob).toContain('event_type="npm-publish"')
    expect(npmDispatchJob).toContain('client_payload[version]="${VERSION}"')
    expect(npmDispatchJob).toContain('contents: write')
    expect(releaseWorkflow).not.toContain('actions/workflows/npm-publish.yml/dispatches')
  })

  it('accepts repository_dispatch payloads in npm-publish.yml', () => {
    const npmWorkflow = readWorkflow('npm-publish.yml')

    expect(npmWorkflow).toContain('repository_dispatch:')
    expect(npmWorkflow).toContain('types: [npm-publish]')
    expect(npmWorkflow).toContain(
      'CLIENT_PAYLOAD_VERSION: ${{ github.event.client_payload.version }}'
    )
    expect(npmWorkflow).toContain('"repository_dispatch"')
  })

  it('removes setup-node token auth before trusted publishing', () => {
    const npmWorkflow = readWorkflow('npm-publish.yml')

    expect(npmWorkflow).toContain(
      'NPM_CONFIG_USERCONFIG="${RUNNER_TEMP}/npm-trusted-publish.npmrc"'
    )
    expect(npmWorkflow).toContain('unset NODE_AUTH_TOKEN')
    expect(npmWorkflow).not.toContain('registry-url:')
    expect(npmWorkflow).not.toContain('_authToken')
    expect(npmWorkflow).toContain('Publish to npm')
  })
})
