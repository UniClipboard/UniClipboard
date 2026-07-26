import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const unsupportedSyntax = [
  { marker: '(?<!', syntax: 'negative regular expression lookbehind' },
  { marker: '(?<=', syntax: 'positive regular expression lookbehind' },
]

function listJavaScriptFiles(directory) {
  return fs
    .readdirSync(directory, { recursive: true, withFileTypes: true })
    .filter(entry => entry.isFile() && /\.m?js$/.test(entry.name))
    .map(entry => path.join(entry.parentPath, entry.name))
    .sort()
}

export function findUnsupportedJavaScript(directory) {
  const findings = []

  for (const file of listJavaScriptFiles(directory)) {
    const content = fs.readFileSync(file, 'utf8')
    for (const { marker, syntax } of unsupportedSyntax) {
      if (content.includes(marker)) findings.push({ file, syntax })
    }
  }

  return findings
}

export function checkMacOSCompatibility(directory) {
  const findings = findUnsupportedJavaScript(directory)
  if (findings.length === 0) return

  const details = findings
    .map(({ file, syntax }) => `- ${path.relative(process.cwd(), file)}: ${syntax}`)
    .join('\n')
  throw new Error(`Built JavaScript is incompatible with macOS 12.5:\n${details}`)
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  const directory = path.resolve(process.argv[2] ?? 'dist')
  try {
    checkMacOSCompatibility(directory)
    console.log(`macOS 12.5 compatibility check passed: ${path.relative(process.cwd(), directory)}`)
  } catch (error) {
    console.error(error instanceof Error ? error.message : error)
    process.exitCode = 1
  }
}
