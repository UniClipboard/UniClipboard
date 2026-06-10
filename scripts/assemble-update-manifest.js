#!/usr/bin/env node

/**
 * Assemble Update Manifest Script
 *
 * Usage:
 *   node scripts/assemble-update-manifest.js \
 *     --version 0.1.0-alpha.1 \
 *     --artifacts-dir release-assets \
 *     --output updates/alpha.json \
 *     --base-url https://github.com/UniClipboard/UniClipboard/releases/download/v0.1.0-alpha.1
 *
 * Options:
 *   --version <ver>           Release version (required)
 *   --artifacts-dir <path>    Directory containing .sig files (required)
 *   --output <path>           Output JSON file path (required)
 *   --base-url <url>          GitHub release download base URL (required)
 *   --test                    Dry-run with mock data, output to stdout
 *   --notes-file <path>       Changelog file to include as release notes
 *   --zh-notes-file <path>    Chinese changelog file to include as release notes
 *
 * Pinned announcement:
 *   If `announcement.md` / `announcement.zh.md` exist next to the notes files,
 *   their content is prepended to the corresponding release notes. Use this to
 *   pin an important notice (e.g. "auto-update is broken on your version,
 *   download manually") at the top of the update dialog for every release.
 *   Delete the files to stop pinning.
 */

import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

// Parse command line arguments
function parseArgs() {
  const args = process.argv.slice(2)
  const options = {
    version: null,
    artifactsDir: null,
    output: null,
    baseUrl: null,
    test: false,
  }

  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--version' && args[i + 1]) {
      options.version = args[i + 1]
      i++
    } else if (args[i] === '--artifacts-dir' && args[i + 1]) {
      options.artifactsDir = args[i + 1]
      i++
    } else if (args[i] === '--output' && args[i + 1]) {
      options.output = args[i + 1]
      i++
    } else if (args[i] === '--base-url' && args[i + 1]) {
      options.baseUrl = args[i + 1]
      i++
    } else if (args[i] === '--notes-file' && args[i + 1]) {
      options.notesFile = args[i + 1]
      i++
    } else if (args[i] === '--zh-notes-file' && args[i + 1]) {
      options.zhNotesFile = args[i + 1]
      i++
    } else if (args[i] === '--test') {
      options.test = true
    }
  }

  return options
}

/**
 * Determine platform from .sig filename/path.
 *
 * Mapping rules:
 *   - aarch64 + .app.tar.gz.sig  → darwin-aarch64
 *   - x64/x86_64 + .app.tar.gz.sig → darwin-x86_64
 *   - .AppImage.sig or .AppImage.tar.gz.sig → linux-x86_64
 *   - arm64/aarch64 + .exe.sig / .nsis.zip.sig / .msi.zip.sig → windows-aarch64
 *   - .exe.sig / .nsis.zip.sig / .msi.zip.sig (otherwise) → windows-x86_64
 *
 * The Windows arch split matters: the Tauri updater requests its platform key
 * as `windows-<arch>`, so an arm64 NSIS sig MUST map to `windows-aarch64` or it
 * would collide with the x64 entry under `windows-x86_64` and one would clobber
 * the other (whichever scanned last), leaving one arch unable to self-update.
 *
 * Returns null if no match.
 */
function detectPlatform(filePath) {
  const normalized = filePath.replaceAll('\\', '/').toLowerCase()

  if (normalized.endsWith('.app.tar.gz.sig')) {
    if (normalized.includes('aarch64-apple-darwin') || normalized.includes('aarch64')) {
      return 'darwin-aarch64'
    }
    if (
      normalized.includes('x86_64-apple-darwin') ||
      normalized.includes('x64') ||
      normalized.includes('x86_64')
    ) {
      return 'darwin-x86_64'
    }
    return null
  }

  if (normalized.endsWith('.appimage.tar.gz.sig') || normalized.endsWith('.appimage.sig')) {
    return 'linux-x86_64'
  }

  if (
    normalized.endsWith('.nsis.zip.sig') ||
    normalized.endsWith('.msi.zip.sig') ||
    normalized.endsWith('.exe.sig')
  ) {
    if (
      normalized.includes('aarch64-pc-windows') ||
      normalized.includes('aarch64') ||
      normalized.includes('arm64')
    ) {
      return 'windows-aarch64'
    }
    return 'windows-x86_64'
  }

  return null
}

function candidatePriority(filePath) {
  const normalized = filePath.replaceAll('\\', '/').toLowerCase()

  if (normalized.endsWith('.app.tar.gz.sig')) return 40
  if (normalized.endsWith('.appimage.tar.gz.sig')) return 30
  if (normalized.endsWith('.appimage.sig')) return 20
  if (normalized.endsWith('.nsis.zip.sig')) return 30
  if (normalized.endsWith('.exe.sig')) return 20
  if (normalized.endsWith('.msi.zip.sig')) return 10

  return 0
}

/**
 * Recursively list files under a directory, returning paths relative to the root directory.
 */
function listFilesRecursively(rootDir, currentDir = rootDir) {
  const entries = fs.readdirSync(currentDir, { withFileTypes: true })
  const files = []

  for (const entry of entries) {
    const fullPath = path.join(currentDir, entry.name)
    if (entry.isDirectory()) {
      files.push(...listFilesRecursively(rootDir, fullPath))
      continue
    }
    if (entry.isFile()) {
      files.push(path.relative(rootDir, fullPath))
    }
  }

  return files
}

/**
 * Scan artifacts directory for .sig files and build platform map.
 * @param {string} artifactsDir - Directory containing .sig files
 * @param {string} baseUrl - Base URL for download links
 * @param {boolean} [silent=false] - Suppress diagnostic output to stderr
 */
function scanArtifacts(artifactsDir, baseUrl, silent = false) {
  const selectedByPlatform = {}

  const files = listFilesRecursively(artifactsDir)
  const sigFiles = files.filter(f => f.toLowerCase().endsWith('.sig')).sort()

  if (sigFiles.length === 0 && !silent) {
    process.stderr.write(`Warning: No .sig files found in ${artifactsDir}\n`)
  }

  for (const sigFile of sigFiles) {
    const platform = detectPlatform(sigFile)
    if (!platform) {
      if (!silent) {
        process.stderr.write(`Skipping unrecognized .sig file: ${sigFile}\n`)
      }
      continue
    }

    const sigPath = path.join(artifactsDir, sigFile)
    const signature = fs.readFileSync(sigPath, 'utf8').trim()
    const priority = candidatePriority(sigFile)

    // The artifact filename is the sig filename without the trailing .sig
    const artifactFilename = path.basename(sigFile.slice(0, -4)) // remove ".sig"
    const url = `${baseUrl}/${artifactFilename}`

    const existing = selectedByPlatform[platform]
    if (existing && existing.priority > priority) {
      if (!silent) {
        process.stderr.write(
          `  [${platform}] keep ${existing.sigFile}, skip lower-priority ${sigFile}\n`
        )
      }
      continue
    }

    selectedByPlatform[platform] = { signature, url, priority, sigFile }

    if (!silent) {
      process.stderr.write(`  [${platform}] ${sigFile} -> ${url}\n`)
    }
  }

  const platforms = {}
  for (const [platform, item] of Object.entries(selectedByPlatform)) {
    platforms[platform] = { signature: item.signature, url: item.url }
  }

  return platforms
}

/**
 * Assemble the combined update manifest JSON.
 */
function assembleManifest(version, platforms, notes = '', zhNotes = '') {
  const pubDate = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')

  const combinedNotes = zhNotes ? notes + '\n\n<!-- zh -->\n\n' + zhNotes : notes

  return {
    version,
    notes: combinedNotes,
    pub_date: pubDate,
    platforms,
  }
}

/**
 * Create mock .sig files in a temp directory for --test mode.
 */
function createMockArtifacts() {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'assemble-manifest-test-'))

  const mockFiles = {
    'uniclipboard_0.1.0-alpha.1_aarch64.app.tar.gz.sig':
      'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUldRVEFBQUFBQUFBQUFBQSttb2NrLXNpZ25hdHVyZS1hYXJjaDY0',
    'uniclipboard_0.1.0-alpha.1_x64.app.tar.gz.sig':
      'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUldRVEFBQUFBQUFBQUFBQSttb2NrLXNpZ25hdHVyZS14NjQ=',
    'uniclipboard_0.1.0-alpha.1_amd64.AppImage.tar.gz.sig':
      'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUldRVEFBQUFBQUFBQUFBQSttb2NrLXNpZ25hdHVyZS1saW51eA==',
    'uniclipboard_0.1.0-alpha.1_x64-setup.nsis.zip.sig':
      'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUldRVEFBQUFBQUFBQUFBQSttb2NrLXNpZ25hdHVyZS13aW5kb3dz',
    'uniclipboard_0.1.0-alpha.1_arm64-setup.nsis.zip.sig':
      'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUldRVEFBQUFBQUFBQUFBQSttb2NrLXNpZ25hdHVyZS13aW5kb3dzLWFybTY0',
  }

  for (const [filename, content] of Object.entries(mockFiles)) {
    fs.writeFileSync(path.join(tmpDir, filename), content, 'utf8')
  }

  return tmpDir
}

/**
 * Read release notes from a changelog file, prepending the pinned
 * announcement (`announcement.md` / `announcement.zh.md` in the same
 * directory) when one exists.
 */
function readNotesWithAnnouncement(notesFile, announcementName, silent) {
  let notes = ''
  if (notesFile && fs.existsSync(notesFile)) {
    notes = fs.readFileSync(notesFile, 'utf8').trim()
  }

  if (!notesFile) return notes

  const announcementPath = path.join(path.dirname(notesFile), announcementName)
  if (!fs.existsSync(announcementPath)) return notes

  const announcement = fs.readFileSync(announcementPath, 'utf8').trim()
  if (!announcement) return notes

  if (!silent) {
    process.stderr.write(`Prepending pinned announcement from ${announcementPath}\n`)
  }
  return notes ? announcement + '\n\n' + notes : announcement
}

function main() {
  const options = parseArgs()

  // Read notes files if provided
  const notes = readNotesWithAnnouncement(options.notesFile, 'announcement.md', options.test)
  const zhNotes = readNotesWithAnnouncement(options.zhNotesFile, 'announcement.zh.md', options.test)

  if (options.test) {
    // --test mode: create mock data and output valid JSON to stdout only.
    // All diagnostic output is suppressed so that piped JSON parsing works cleanly
    // (e.g. node scripts/assemble-update-manifest.js --test 2>&1 | node -e "JSON.parse(d)").
    const version = options.version || '0.1.0-alpha.1'
    const baseUrl =
      options.baseUrl ||
      `https://github.com/UniClipboard/UniClipboard/releases/download/v${version}`

    const tmpDir = createMockArtifacts()

    try {
      const platforms = scanArtifacts(tmpDir, baseUrl, true)
      const manifest = assembleManifest(version, platforms, notes, zhNotes)
      const json = JSON.stringify(manifest, null, 2)

      process.stdout.write(json + '\n')
    } finally {
      // Clean up temp dir
      fs.rmSync(tmpDir, { recursive: true, force: true })
    }

    return
  }

  // Normal mode: validate required args
  const missing = []
  if (!options.version) missing.push('--version')
  if (!options.artifactsDir) missing.push('--artifacts-dir')
  if (!options.output) missing.push('--output')
  if (!options.baseUrl) missing.push('--base-url')

  if (missing.length > 0) {
    process.stderr.write(`Error: Missing required arguments: ${missing.join(', ')}\n`)
    process.stderr.write(
      'Usage: node scripts/assemble-update-manifest.js --version <ver> --artifacts-dir <path> --output <path> --base-url <url>\n'
    )
    process.exit(1)
  }

  const artifactsDir = path.resolve(options.artifactsDir)

  if (!fs.existsSync(artifactsDir)) {
    process.stderr.write(`Error: Artifacts directory does not exist: ${artifactsDir}\n`)
    process.exit(1)
  }

  process.stderr.write(`Assembling update manifest for v${options.version}\n`)
  process.stderr.write(`Artifacts dir: ${artifactsDir}\n`)
  process.stderr.write(`Base URL: ${options.baseUrl}\n\n`)

  const platforms = scanArtifacts(artifactsDir, options.baseUrl)

  if (Object.keys(platforms).length === 0) {
    process.stderr.write('Error: No recognized platform artifacts found.\n')
    process.exit(1)
  }

  const manifest = assembleManifest(options.version, platforms, notes, zhNotes)
  const json = JSON.stringify(manifest, null, 2)

  // Ensure output directory exists
  const outputPath = path.resolve(options.output)
  const outputDir = path.dirname(outputPath)
  if (!fs.existsSync(outputDir)) {
    fs.mkdirSync(outputDir, { recursive: true })
  }

  fs.writeFileSync(outputPath, json + '\n', 'utf8')

  process.stderr.write(`\nManifest written to: ${outputPath}\n`)
  process.stderr.write(`Platforms included: ${Object.keys(platforms).join(', ')}\n`)
}

main()
