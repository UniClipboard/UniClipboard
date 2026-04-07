#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

function changelogPaths(version) {
  return {
    english: path.join('docs', 'changelog', `${version}.md`),
    chinese: path.join('docs', 'changelog', `${version}.zh.md`),
  }
}

function ensureChangelogDir() {
  fs.mkdirSync(path.join('docs', 'changelog'), { recursive: true })
}

function englishDraft(version, date) {
  return `## ${version} - ${date}

### Features

<!-- Replace with user-facing additions or remove this section if not needed. -->

### Fixes

<!-- Replace with user-facing fixes or remove this section if not needed. -->
`
}

function chineseDraft(version, date) {
  return `## ${version} - ${date}

### Features

<!-- 填写本次版本中用户能直接感受到的新增或改进；若没有可删除整个小节。 -->

### Fixes

<!-- 填写本次版本中修复的用户可感知问题；若没有可删除整个小节。 -->
`
}

export function createChangelogDraftFiles({ version, date }) {
  ensureChangelogDir()
  const paths = changelogPaths(version)

  if (!fs.existsSync(paths.english)) {
    fs.writeFileSync(paths.english, englishDraft(version, date), 'utf8')
  }

  if (!fs.existsSync(paths.chinese)) {
    fs.writeFileSync(paths.chinese, chineseDraft(version, date), 'utf8')
  }

  return paths
}

function assertFinalizedContent(filePath, version) {
  const content = fs.readFileSync(filePath, 'utf8')
  const placeholderMarkers = [
    'Release notes are not available yet.',
    'Release notes pending.',
    'Pending release notes.',
    'No installer artifacts found for this release.',
    'No CLI artifacts found for this release.',
    '<!--',
  ]

  if (!content.startsWith(`## ${version} - `)) {
    throw new Error(`Unexpected changelog heading in ${filePath}`)
  }

  if (placeholderMarkers.some(marker => content.includes(marker))) {
    throw new Error(`Changelog file ${filePath} still contains unfinished placeholder content`)
  }
}

export function validateChangelogFiles({ version }) {
  const paths = changelogPaths(version)
  const missing = Object.values(paths).filter(filePath => !fs.existsSync(filePath))

  if (missing.length > 0) {
    throw new Error(`Missing changelog files: ${missing.join(', ')}`)
  }

  assertFinalizedContent(paths.english, version)
  assertFinalizedContent(paths.chinese, version)

  return paths
}

function parseArgs(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv
  const options = { command }

  for (let index = 0; index < rest.length; index += 1) {
    const arg = rest[index]
    const next = rest[index + 1]

    if (!arg.startsWith('--')) {
      continue
    }

    options[arg.slice(2)] = next
    index += 1
  }

  return options
}

function ensureOption(options, key) {
  if (!options[key]) {
    throw new Error(`Missing required option: --${key}`)
  }
}

function main() {
  const options = parseArgs()

  if (options.command === 'init') {
    ensureOption(options, 'version')
    ensureOption(options, 'date')
    createChangelogDraftFiles({
      version: options.version,
      date: options.date,
    })
    return
  }

  if (options.command === 'validate') {
    ensureOption(options, 'version')
    validateChangelogFiles({
      version: options.version,
    })
    return
  }

  throw new Error(
    'Usage: node scripts/changelog-draft.js <init|validate> --version <ver> [--date <YYYY-MM-DD>]'
  )
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
