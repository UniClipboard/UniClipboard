#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

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

function readRequiredFile(filePath, label) {
  if (!filePath || !fs.existsSync(filePath)) {
    throw new Error(`${label} file does not exist: ${filePath ?? '(missing)'}`)
  }
  return fs.readFileSync(filePath, 'utf8').trim()
}

export function buildRegistration({ version, channel, manifest, artifactsDir, source }) {
  const notesSeparator = '\n\n<!-- zh -->\n\n'
  const separatorIndex = manifest.notes.indexOf(notesSeparator)
  if (separatorIndex < 0) {
    throw new Error('Manifest notes must contain the bilingual separator')
  }
  const notesEn = manifest.notes.slice(0, separatorIndex)
  const notesZh = manifest.notes.slice(separatorIndex + notesSeparator.length)
  const artifacts = Object.entries(manifest.platforms).map(([platform, entry]) => {
    const filename = decodeURIComponent(new URL(entry.url).pathname.split('/').at(-1))
    const artifactPath = path.join(artifactsDir, filename)
    if (!fs.existsSync(artifactPath)) {
      throw new Error(`Artifact selected for ${platform} does not exist: ${artifactPath}`)
    }
    return {
      platform,
      filename,
      r2Key: `artifacts/v${version}/${filename}`,
      downloadUrl: entry.url,
      size: fs.statSync(artifactPath).size,
      signature: entry.signature,
    }
  })

  if (artifacts.length === 0) throw new Error('Registration requires at least one artifact')

  return {
    product: 'desktop',
    version,
    tagName: `v${version}`,
    prerelease: channel !== 'stable',
    publishedAt: manifest.pub_date,
    source,
    notes: { en: notesEn, zh: notesZh },
    artifacts,
  }
}

function main() {
  const options = parseArgs()
  const required = ['version', 'channel', 'manifest', 'artifacts-dir', 'output']
  const missing = required.filter(key => !options[key])
  if (missing.length > 0) throw new Error(`Missing required options: ${missing.join(', ')}`)

  const manifest = JSON.parse(readRequiredFile(options.manifest, 'Manifest'))
  const registration = buildRegistration({
    version: options.version,
    channel: options.channel,
    manifest,
    artifactsDir: options['artifacts-dir'],
    source: options.source ?? 'github-actions',
  })

  const outputPath = path.resolve(options.output)
  fs.mkdirSync(path.dirname(outputPath), { recursive: true })
  fs.writeFileSync(outputPath, `${JSON.stringify(registration, null, 2)}\n`, 'utf8')
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
