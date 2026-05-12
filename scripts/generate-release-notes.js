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

function buildInstallerTable({ artifactsDir, baseUrl }) {
  // Tauri v2 Linux 产物命名约定:
  //   .deb       — `<lower-product>_<version>_<amd64|arm64>.deb`        (Debian arch)
  //   .rpm       — `<ProductName>-<version>-1.<x86_64|aarch64>.rpm`     (RPM arch)
  //   .AppImage  — `<lower-product>_<version>_<amd64|aarch64>.AppImage` (混用)
  // 用 isArm 判别同时覆盖 arm64/aarch64 两种写法。
  const isArm = file => /aarch64|arm64/.test(file)
  const isX64 = file => /x86_64|x64|amd64/.test(file)

  const macosArm64 = findFirstFile(artifactsDir, file => file.endsWith('.dmg') && isArm(file))
  const macosX64 = findFirstFile(artifactsDir, file => file.endsWith('.dmg') && isX64(file))
  const linuxDebX64 = findFirstFile(artifactsDir, file => file.endsWith('.deb') && isX64(file))
  const linuxDebArm = findFirstFile(artifactsDir, file => file.endsWith('.deb') && isArm(file))
  const linuxRpmX64 = findFirstFile(artifactsDir, file => file.endsWith('.rpm') && isX64(file))
  const linuxRpmArm = findFirstFile(artifactsDir, file => file.endsWith('.rpm') && isArm(file))
  const linuxAppImageX64 = findFirstFile(
    artifactsDir,
    file => file.endsWith('.AppImage') && isX64(file)
  )
  const linuxAppImageArm = findFirstFile(
    artifactsDir,
    file => file.endsWith('.AppImage') && isArm(file)
  )
  const windowsExe = findFirstFile(artifactsDir, file => file.endsWith('.exe'))

  const makeRow = (platform, arch, fileName) =>
    `| ${platform} | ${arch} | [${fileName}](${baseUrl}/${fileName}) |`

  const rows = []
  if (macosArm64) rows.push(makeRow('macOS', 'Apple Silicon (M1/M2/M3)', macosArm64))
  if (macosX64) rows.push(makeRow('macOS', 'Intel', macosX64))
  if (linuxDebX64) rows.push(makeRow('Linux', 'Debian/Ubuntu x86_64 (.deb)', linuxDebX64))
  if (linuxDebArm) rows.push(makeRow('Linux', 'Debian/Ubuntu aarch64 (.deb)', linuxDebArm))
  if (linuxRpmX64) rows.push(makeRow('Linux', 'Fedora/RHEL x86_64 (.rpm)', linuxRpmX64))
  if (linuxRpmArm) rows.push(makeRow('Linux', 'Fedora/RHEL aarch64 (.rpm)', linuxRpmArm))
  if (linuxAppImageX64) rows.push(makeRow('Linux', 'AppImage x86_64', linuxAppImageX64))
  if (linuxAppImageArm) rows.push(makeRow('Linux', 'AppImage aarch64', linuxAppImageArm))
  if (windowsExe) rows.push(makeRow('Windows', 'x86_64', windowsExe))

  if (rows.length === 0) {
    return 'No installer artifacts found for this release.'
  }

  return '| Platform | Architecture | Download |\n| --- | --- | --- |\n' + rows.join('\n')
}

function buildCliInstallerTable({ artifactsDir, baseUrl }) {
  const isArm = file => /aarch64|arm64/.test(file)
  const isX64 = file => /x86_64|x64|amd64/.test(file)

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
  const linuxX64 = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') &&
      file.includes('linux-musl') &&
      isX64(file) &&
      file.endsWith('.tar.gz')
  )
  const linuxArm64 = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') &&
      file.includes('linux-musl') &&
      isArm(file) &&
      file.endsWith('.tar.gz')
  )
  const windows = findFirstFile(
    artifactsDir,
    file =>
      file.startsWith('uniclipboard-cli-') && file.includes('windows-msvc') && file.endsWith('.zip')
  )

  const makeRow = (platform, arch, fileName) =>
    `| ${platform} | ${arch} | [${fileName}](${baseUrl}/${fileName}) |`

  const rows = []
  if (macosArm64) rows.push(makeRow('macOS', 'Apple Silicon (M1/M2/M3)', macosArm64))
  if (macosX64) rows.push(makeRow('macOS', 'Intel', macosX64))
  if (linuxX64) rows.push(makeRow('Linux', 'x86_64', linuxX64))
  if (linuxArm64) rows.push(makeRow('Linux', 'aarch64', linuxArm64))
  if (windows) rows.push(makeRow('Windows', 'x86_64', windows))

  if (rows.length === 0) {
    return 'No CLI artifacts found for this release.'
  }

  return '| Platform | Architecture | Download |\n| --- | --- | --- |\n' + rows.join('\n')
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

  const installerTable = buildInstallerTable({ artifactsDir, baseUrl })
  const cliInstallerTable = buildCliInstallerTable({ artifactsDir, baseUrl })
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
      INSTALLER_TABLE: installerTable,
      CLI_INSTALLER_TABLE: cliInstallerTable,
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
