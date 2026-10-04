#!/usr/bin/env node
// Guards mirror-desktop-gitcode.yml's `mirror` job against a draft release
// or an inconsistent tag_name/version/channel, before anything downloads
// or uploads. The automatic dispatch paths already construct these
// consistently; manual workflow_dispatch does not.

import process from 'node:process'

export const CHANNELS = ['stable', 'alpha', 'beta', 'rc']

export function deriveChannelFromVersion(version) {
  const match = version.match(/-([a-zA-Z]+)/)
  return match ? match[1] : 'stable'
}

export function validateMirrorRequest(release, { tagName, version, channel }) {
  const errors = []

  if (release.draft) {
    errors.push(`release ${tagName} is still a draft; refusing to mirror an unpublished release`)
  }
  if (release.tag_name !== tagName) {
    errors.push(`GitHub release tag_name "${release.tag_name}" does not match the requested tag_name "${tagName}"`)
  }
  if (!CHANNELS.includes(channel)) {
    errors.push(`channel "${channel}" is not one of ${CHANNELS.join('/')}`)
  }

  const expectedTagName = `v${version}`
  if (tagName !== expectedTagName) {
    errors.push(`tag_name "${tagName}" does not match version "${version}" (expected "${expectedTagName}")`)
  }

  const expectedChannel = deriveChannelFromVersion(version)
  if (channel !== expectedChannel) {
    errors.push(`channel "${channel}" does not match the channel implied by version "${version}" (expected "${expectedChannel}")`)
  }

  return errors
}

export async function fetchRelease({ repo, tagName, token, fetchImpl = fetch }) {
  const response = await fetchImpl(`https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tagName)}`, {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
    },
  })
  if (!response.ok) {
    throw new Error(`GET repos/${repo}/releases/tags/${tagName} failed: ${response.status} ${await response.text()}`)
  }
  return response.json()
}

function parseArgs(argv = process.argv.slice(2)) {
  const options = {}
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    const value = argv[index + 1]
    if (key?.startsWith('--') && value) {
      options[key.slice(2)] = value
      index += 1
    }
  }
  return options
}

async function main() {
  const options = parseArgs()
  const required = ['repo', 'tag-name', 'version', 'channel']
  const missing = required.filter((key) => !options[key])
  if (missing.length > 0) throw new Error(`Missing required options: ${missing.join(', ')}`)

  const token = process.env.GH_TOKEN ?? process.env.GITHUB_TOKEN
  if (!token) throw new Error('GH_TOKEN (or GITHUB_TOKEN) must be set')

  const tagName = options['tag-name']
  const release = await fetchRelease({ repo: options.repo, tagName, token })
  const errors = validateMirrorRequest(release, { tagName, version: options.version, channel: options.channel })

  if (errors.length > 0) {
    for (const error of errors) console.error(`::error::${error}`)
    throw new Error(`mirror request rejected for ${tagName}: ${errors.join('; ')}`)
  }

  console.log(`mirror request accepted for ${tagName} (${options.channel})`)
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  })
}
