#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

function parseArgs(argv = process.argv.slice(2)) {
  const options = {
    version: null,
    repo: null,
    previousTag: null,
    channel: 'stable',
    isPrerelease: false,
    artifactsDir: null,
    template: null,
    output: null,
    docsBaseUrl: null,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    const next = argv[index + 1]

    if (arg === '--version' && next) {
      options.version = next
      index += 1
    } else if (arg === '--repo' && next) {
      options.repo = next
      index += 1
    } else if (arg === '--previous-tag' && next) {
      options.previousTag = next
      index += 1
    } else if (arg === '--channel' && next) {
      options.channel = next
      index += 1
    } else if (arg === '--is-prerelease' && next) {
      options.isPrerelease = next === 'true'
      index += 1
    } else if (arg === '--artifacts-dir' && next) {
      options.artifactsDir = next
      index += 1
    } else if (arg === '--template' && next) {
      options.template = next
      index += 1
    } else if (arg === '--output' && next) {
      options.output = next
      index += 1
    } else if (arg === '--docs-base-url' && next) {
      options.docsBaseUrl = next
      index += 1
    }
  }

  return options
}

function ensureRequired(options) {
  const required = ['version', 'repo', 'previousTag', 'artifactsDir', 'template', 'output']
  const missing = required.filter(key => !options[key])
  if (missing.length > 0) {
    throw new Error(`Missing required options: ${missing.join(', ')}`)
  }
}

function formatWarning(message, filePath) {
  if (filePath) {
    return `::warning file=${filePath}::${message}`
  }
  return `::warning::${message}`
}

function appendSummary(line) {
  const summaryPath = process.env.GITHUB_STEP_SUMMARY
  if (!summaryPath) {
    return
  }
  fs.appendFileSync(summaryPath, `${line}\n`, 'utf8')
}

function emitWarning(message, filePath) {
  console.warn(formatWarning(message, filePath))
  appendSummary(`- Warning: ${message}${filePath ? ` (${filePath})` : ''}`)
}

function findFirstFile(artifactsDir, predicate) {
  if (!fs.existsSync(artifactsDir)) {
    return ''
  }

  const entries = fs.readdirSync(artifactsDir, { withFileTypes: true })
  const files = entries
    .filter(entry => entry.isFile())
    .map(entry => entry.name)
    .sort((left, right) => left.localeCompare(right))

  return files.find(predicate) || ''
}

function buildInstallerLines({ artifactsDir, baseUrl }) {
  const macosArm64 = findFirstFile(
    artifactsDir,
    file => file.endsWith('.dmg') && (file.includes('aarch64') || file.includes('arm64'))
  )
  const macosX64 = findFirstFile(
    artifactsDir,
    file => file.endsWith('.dmg') && (file.includes('x64') || file.includes('x86_64'))
  )
  const linuxDeb = findFirstFile(artifactsDir, file => file.endsWith('.deb'))
  const linuxAppImage = findFirstFile(artifactsDir, file => file.endsWith('.AppImage'))
  const windowsExe = findFirstFile(artifactsDir, file => file.endsWith('.exe'))

  const makeLink = (label, fileName) => `- ${label}: [${fileName}](${baseUrl}/${fileName})`
  const fallback = '- Not available for this release.'

  return {
    macos:
      [
        macosArm64 ? makeLink('**Apple Silicon (M1/M2/M3)**', macosArm64) : '',
        macosX64 ? makeLink('**Intel**', macosX64) : '',
      ]
        .filter(Boolean)
        .join('\n') || fallback,
    linux:
      [
        linuxDeb ? makeLink('**Debian/Ubuntu**', linuxDeb) : '',
        linuxAppImage ? makeLink('**AppImage**', linuxAppImage) : '',
      ]
        .filter(Boolean)
        .join('\n') || fallback,
    windows: windowsExe ? makeLink('**NSIS Installer**', windowsExe) : fallback,
  }
}

function buildCliInstallerLines({ artifactsDir, baseUrl }) {
  const macosArm64 = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') &&
      file.includes('aarch64-apple-darwin') &&
      file.endsWith('.tar.gz')
  )
  const macosX64 = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') &&
      file.includes('x86_64-apple-darwin') &&
      file.endsWith('.tar.gz')
  )
  const linux = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') && file.includes('linux-gnu') && file.endsWith('.tar.gz')
  )
  const windows = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') && file.includes('windows-msvc') && file.endsWith('.zip')
  )

  const makeLink = (label, fileName) => `- ${label}: [${fileName}](${baseUrl}/${fileName})`
  const fallback = '- Not available for this release.'

  return {
    macos:
      [
        macosArm64 ? makeLink('**Apple Silicon (M1/M2/M3)**', macosArm64) : '',
        macosX64 ? makeLink('**Intel**', macosX64) : '',
      ]
        .filter(Boolean)
        .join('\n') || fallback,
    linux: linux ? makeLink('**x86_64**', linux) : fallback,
    windows: windows ? makeLink('**x86_64**', windows) : fallback,
  }
}

function readChangelogSection(filePath, fallbackTitle = "## What's Changed") {
  if (!fs.existsSync(filePath)) {
    return `${fallbackTitle}\n\nRelease notes are not available yet.`
  }

  return fs.readFileSync(filePath, 'utf8').trim()
}

function buildChangelogLinks({ version, docsBaseUrl, englishExists, chineseExists }) {
  const lines = []

  if (englishExists) {
    const englishUrl = `${docsBaseUrl}/${version}.md`
    lines.push(`- English changelog: [${version}.md](${englishUrl})`)
  }

  if (chineseExists) {
    const chineseUrl = `${docsBaseUrl}/${version}.zh.md`
    lines.push(`- 中文变更日志: [${version}.zh.md](${chineseUrl})`)
  }

  if (lines.length === 0) {
    return ''
  }

  return `\n**Changelog Files**\n\n${lines.join('\n')}`
}

function buildPrereleaseWarning(isPrerelease, channel) {
  if (!isPrerelease) {
    return ''
  }

  return `\n## ⚠️ Prerelease Warning\n\nThis is a **${channel}** release and may contain bugs or incomplete features.\nNot recommended for production use. Please report issues on GitHub.\n`
}

function renderTemplate(template, replacements) {
  return Object.entries(replacements).reduce(
    (content, [key, value]) => content.replaceAll(`{{${key}}}`, value),
    template
  )
}

export function generateReleaseNotes(options) {
  ensureRequired(options)

  const version = options.version
  const repo = options.repo
  const previousTag = options.previousTag
  const templatePath = options.template
  const artifactsDir = options.artifactsDir
  const outputPath = options.output
  const baseUrl = `https://github.com/${repo}/releases/download/v${version}`
  const docsBaseUrl =
    options.docsBaseUrl || `https://github.com/${repo}/blob/v${version}/docs/changelog`
  const englishPath = path.join('docs', 'changelog', `${version}.md`)
  const chinesePath = path.join('docs', 'changelog', `${version}.zh.md`)
  const englishExists = fs.existsSync(englishPath)
  const chineseExists = fs.existsSync(chinesePath)

  appendSummary('### Release Notes Sources')
  appendSummary(`- Template: ${templatePath}`)
  appendSummary(`- English changelog: ${englishExists ? englishPath : 'missing'}`)
  appendSummary(`- Chinese changelog: ${chineseExists ? chinesePath : 'missing'}`)

  if (!englishExists) {
    emitWarning(`Release changelog file not found for version ${version}`, englishPath)
  }
  if (!chineseExists) {
    emitWarning(`Chinese release changelog file not found for version ${version}`, chinesePath)
  }

  const installers = buildInstallerLines({ artifactsDir, baseUrl })
  const cliInstallers = buildCliInstallerLines({ artifactsDir, baseUrl })
  const template = fs.readFileSync(templatePath, 'utf8')
  const rendered =
    renderTemplate(template, {
      VERSION: version,
      REPO: repo,
      PREVIOUS_TAG: previousTag,
      CHANGELOG_SECTION: readChangelogSection(englishPath),
      CHANGELOG_LINKS_SECTION: buildChangelogLinks({
        version,
        docsBaseUrl,
        englishExists,
        chineseExists,
      }),
      IS_PRERELEASE_WARNING: buildPrereleaseWarning(options.isPrerelease, options.channel),
      MACOS_INSTALLERS: installers.macos,
      LINUX_INSTALLERS: installers.linux,
      WINDOWS_INSTALLERS: installers.windows,
      CLI_MACOS_INSTALLERS: cliInstallers.macos,
      CLI_LINUX_INSTALLERS: cliInstallers.linux,
      CLI_WINDOWS_INSTALLERS: cliInstallers.windows,
      VERIFICATION_SECTION: '',
    }).trim() + '\n'

  fs.writeFileSync(outputPath, rendered, 'utf8')
  return { outputPath, englishExists, chineseExists }
}

function main() {
  try {
    const options = parseArgs()
    generateReleaseNotes(options)
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main()
}
