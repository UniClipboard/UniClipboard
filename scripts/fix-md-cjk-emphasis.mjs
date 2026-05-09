#!/usr/bin/env node
// Insert spaces between CJK characters and Markdown ** emphasis pairs so the
// emphasis renders correctly. CommonMark/remark parsers don't always recognise
// `**bold**` as emphasis when CJK letters are flush against the ** markers.
//
// Examples:
//   通过**空间（Space）**进  →  通过 **空间（Space）** 进
//   **起始加粗**也是中文      →  **起始加粗** 也是中文
//   正文里**这里**应该改      →  正文里 **这里** 应该改
//
// Skips: YAML frontmatter, fenced code blocks, inline code, and `**` markers
// adjacent to non-letter content where the parser already renders correctly.

import { readFileSync, writeFileSync } from 'node:fs'

const HAN_RE = /[一-鿿㐀-䶿]/
const NUL = String.fromCharCode(0)

function transform(content) {
  const stashed = []
  const stash = m => {
    stashed.push(m)
    return `${NUL}${stashed.length - 1}${NUL}`
  }

  let masked = content
  masked = masked.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, stash)
  masked = masked.replace(/```[\s\S]*?```/g, stash)
  masked = masked.replace(/~~~[\s\S]*?~~~/g, stash)
  masked = masked.replace(/`[^`\n]+`/g, stash)

  masked = masked
    .split('\n')
    .map(line => {
      // Skip lines with an odd number of ** — pairing is ambiguous and a
      // naive left-to-right pair-up would mis-handle a stray literal **.
      const stars = (line.match(/\*\*/g) || []).length
      if (stars === 0 || stars % 2 !== 0) return line
      return line.replace(/\*\*([^*\n]+?)\*\*/g, (match, inner, offset, full) => {
        const before = offset > 0 ? full[offset - 1] : ''
        const after = full[offset + match.length] ?? ''
        const prefix = HAN_RE.test(before) ? ' ' : ''
        const suffix = HAN_RE.test(after) ? ' ' : ''
        return `${prefix}**${inner}**${suffix}`
      })
    })
    .join('\n')

  masked = masked.replace(new RegExp(`${NUL}(\\d+)${NUL}`, 'g'), (_, i) => stashed[Number(i)])

  return masked
}

const files = process.argv.slice(2)
for (const file of files) {
  const before = readFileSync(file, 'utf8')
  const after = transform(before)
  if (before !== after) {
    writeFileSync(file, after)
    console.log(`fix-md-cjk-emphasis: ${file}`)
  }
}
