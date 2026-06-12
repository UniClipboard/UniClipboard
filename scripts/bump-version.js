#!/usr/bin/env node

/**
 * Version Bump Script
 *
 * Usage:
 *   node scripts/bump-version.js --type patch --channel alpha
 *   node scripts/bump-version.js --type minor --channel stable
 *   node scripts/bump-version.js --type major --channel beta
 *   node scripts/bump-version.js --to 0.1.0-alpha.2
 *
 * Options:
 *   --to <version>  Set exact semver target (mutually exclusive with --type/--channel)
 *   --type <patch|minor|major>  Version bump type (required unless --to is used)
 *   --channel <stable|alpha|beta|rc>  Release channel (default: stable)
 *   --dry-run  Show what would be changed without writing
 */

import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { bumpVersion, parseSemver } from './bump-version-lib.js'

export function parseArgs(argv = process.argv.slice(2)) {
  const options = {
    to: null,
    type: null,
    channel: 'stable',
    channelProvided: false,
    dryRun: false,
  }

  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--to' && argv[i + 1]) {
      options.to = argv[i + 1]
      i += 1
    } else if (argv[i] === '--type' && argv[i + 1]) {
      options.type = argv[i + 1]
      i += 1
    } else if (argv[i] === '--channel' && argv[i + 1]) {
      options.channel = argv[i + 1]
      options.channelProvided = true
      i += 1
    } else if (argv[i] === '--dry-run') {
      options.dryRun = true
    }
  }

  return options
}

export function updatePackageJson(newVersion, dryRun) {
  const pkgPath = path.join(process.cwd(), 'package.json')
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'))
  const oldVersion = pkg.version

  pkg.version = newVersion

  if (!dryRun) {
    fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8')
  }

  return { path: pkgPath, old: oldVersion, new: newVersion }
}

export function updateTauriConfig(newVersion, dryRun) {
  const configPath = path.join(process.cwd(), 'src-tauri', 'tauri.conf.json')
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8'))
  const oldVersion = config.version

  config.version = newVersion

  if (!dryRun) {
    fs.writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n', 'utf8')
  }

  return { path: configPath, old: oldVersion, new: newVersion }
}

export function updateCargoToml(
  newVersion,
  dryRun,
  relativePath = path.join('src-tauri', 'Cargo.toml')
) {
  const cargoPath = path.join(process.cwd(), relativePath)
  const content = fs.readFileSync(cargoPath, 'utf8')

  // Match ALL lines with version = "..." — covers [package] and [workspace.package]
  const versionLineRegex = /^version\s*=\s*"[^"]+"/gm
  const matches = content.match(versionLineRegex)

  if (!matches || matches.length === 0) {
    throw new Error('Could not find version in Cargo.toml')
  }

  const oldVersion = matches[0].match(/^version\s*=\s*"([^"]+)"/)[1]
  const newContent = content.replace(versionLineRegex, `version = "${newVersion}"`)

  if (!dryRun) {
    fs.writeFileSync(cargoPath, newContent, 'utf8')
  }

  return { path: cargoPath, old: oldVersion, new: newVersion }
}

export function updateCargoLock(newVersion, dryRun) {
  // The cargo workspace lives at the repo root, so Cargo.lock does too; the
  // `uniclipboard` package manifest still lives in src-tauri/Cargo.toml.
  const cargoTomlPath = path.join(process.cwd(), 'src-tauri', 'Cargo.toml')
  const cargoLockPath = path.join(process.cwd(), 'Cargo.lock')

  if (!fs.existsSync(cargoLockPath)) {
    return { path: cargoLockPath, skipped: true, reason: 'Cargo.lock not found' }
  }

  const cargoToml = fs.readFileSync(cargoTomlPath, 'utf8')
  const nameMatch = cargoToml.match(/^name\s*=\s*"([^"]+)"/m)
  if (!nameMatch) {
    throw new Error('Could not find package name in Cargo.toml')
  }

  const packageName = nameMatch[1]
  const escapedPackageName = packageName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const content = fs.readFileSync(cargoLockPath, 'utf8')
  const packageRegex = new RegExp(
    `(\\[\\[package\\]\\]\\nname = "${escapedPackageName}"\\nversion = "([^"]+)")`,
    'm'
  )
  const match = content.match(packageRegex)

  if (!match) {
    throw new Error(`Could not find package '${packageName}' in Cargo.lock`)
  }

  const oldVersion = match[2]
  const newContent = content.replace(
    packageRegex,
    `[[package]]\nname = "${packageName}"\nversion = "${newVersion}"`
  )

  if (!dryRun) {
    fs.writeFileSync(cargoLockPath, newContent, 'utf8')
  }

  return { path: cargoLockPath, old: oldVersion, new: newVersion, skipped: false }
}

export function run(options = parseArgs()) {
  const isExactVersionMode = Boolean(options.to)

  if (isExactVersionMode) {
    if (options.type) {
      throw new Error('--to cannot be used together with --type')
    }
    if (options.channelProvided) {
      throw new Error('--to cannot be used together with --channel')
    }
    parseSemver(options.to)
  } else {
    if (!options.type) {
      throw new Error('--type is required (patch|minor|major)')
    }

    if (!['patch', 'minor', 'major'].includes(options.type)) {
      throw new Error(`Invalid bump type '${options.type}'. Must be patch, minor, or major.`)
    }

    if (!['stable', 'alpha', 'beta', 'rc'].includes(options.channel)) {
      throw new Error(`Invalid channel '${options.channel}'. Must be stable, alpha, beta, or rc.`)
    }
  }

  const pkgPath = path.join(process.cwd(), 'package.json')
  const pkgContent = fs.readFileSync(pkgPath, 'utf8')
  const pkg = JSON.parse(pkgContent)
  const currentVersion = pkg.version
  const newVersion = isExactVersionMode
    ? options.to
    : bumpVersion(currentVersion, options.type, options.channel)

  console.log('\n📦 Version Bump Summary\n')
  console.log(`Current version: ${currentVersion}`)
  if (isExactVersionMode) {
    console.log('Mode:            exact')
  } else {
    console.log(`Bump type:       ${options.type}`)
    console.log(`Channel:         ${options.channel}`)
  }
  console.log(`New version:     ${newVersion}`)

  if (newVersion === currentVersion) {
    console.log('Version change:  unchanged')
  }

  if (options.dryRun) {
    console.log('\n🔍 DRY RUN - No files will be modified\n')
  } else {
    console.log('')
  }

  const packageResult = updatePackageJson(newVersion, options.dryRun)
  console.log(`${options.dryRun ? '[DRY RUN]' : '✓'} ${packageResult.path}`)
  console.log(`  ${packageResult.old} → ${packageResult.new}`)

  const tauriResult = updateTauriConfig(newVersion, options.dryRun)
  console.log(`${options.dryRun ? '[DRY RUN]' : '✓'} ${tauriResult.path}`)
  console.log(`  ${tauriResult.old} → ${tauriResult.new}`)

  // Root workspace manifest: [workspace.package] version, inherited by all
  // workspace member crates via `version.workspace = true`.
  const rootCargoResult = updateCargoToml(newVersion, options.dryRun, 'Cargo.toml')
  console.log(`${options.dryRun ? '[DRY RUN]' : '✓'} ${rootCargoResult.path}`)
  console.log(`  ${rootCargoResult.old} → ${rootCargoResult.new}`)

  // Tauri bin package manifest: [package] version of `uniclipboard`.
  const cargoResult = updateCargoToml(newVersion, options.dryRun)
  console.log(`${options.dryRun ? '[DRY RUN]' : '✓'} ${cargoResult.path}`)
  console.log(`  ${cargoResult.old} → ${cargoResult.new}`)

  const cargoLockResult = updateCargoLock(newVersion, options.dryRun)
  if (cargoLockResult.skipped) {
    console.log(`${options.dryRun ? '[DRY RUN]' : '-'} ${cargoLockResult.path}`)
    console.log(`  skipped: ${cargoLockResult.reason}`)
  } else {
    console.log(`${options.dryRun ? '[DRY RUN]' : '✓'} ${cargoLockResult.path}`)
    console.log(`  ${cargoLockResult.old} → ${cargoLockResult.new}`)
  }

  if (!options.dryRun) {
    console.log('\n✨ Version bump complete!\n')
    console.log('Next steps:')
    console.log('  1. Review the changes: git diff')
    console.log(
      '  2. Commit the changes: git add . && git commit -m "chore: bump version to ' +
        newVersion +
        '"'
    )
    console.log('  3. Push and trigger release workflow\n')
  }

  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `version=${newVersion}\n`)
  }

  return { newVersion }
}

function main() {
  try {
    run()
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    console.error(`\n❌ Error: ${message}\n`)
    process.exit(1)
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main()
}
