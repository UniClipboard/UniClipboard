#!/usr/bin/env node
// 文档一致性巡检：
//   1. zh/ 与 en/ 必须 1:1 对齐（同名 .mdx）
//   2. MDX 中的相对链接 (./foo#bar) / (../foo#bar) 的目标文件存在，且锚点（slug）能在目标文件的 heading 中匹配
//   3. 全文不应再出现裸 autolink <https://...>（MDX 3 会把它当 JSX 解析）
//
// 用法：
//   bun run check:docs
//   或：node docs-site/scripts/check-docs.mjs
//
// 退出码 0 = 全绿；1 = 有问题。

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, normalize, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import GithubSlugger from 'github-slugger'

const __dirname = dirname(fileURLToPath(import.meta.url))
const DOCS_ROOT = resolve(__dirname, '..', 'content', 'docs')
const LANGS = ['en', 'zh']

const errors = []
const warnings = []

function walk(dir) {
  const out = []
  for (const ent of readdirSync(dir)) {
    const p = join(dir, ent)
    const s = statSync(p)
    if (s.isDirectory()) out.push(...walk(p))
    else if (p.endsWith('.mdx')) out.push(p)
  }
  return out
}

// ---------- 1) zh / en 镜像 ----------
const langPages = Object.fromEntries(
  LANGS.map(l => [l, walk(join(DOCS_ROOT, l)).map(p => relative(join(DOCS_ROOT, l), p))])
)

for (const lang of LANGS) {
  const other = LANGS.find(l => l !== lang)
  for (const page of langPages[lang]) {
    if (!langPages[other].includes(page)) {
      errors.push(`mirror: ${lang}/${page} 没有对应的 ${other}/${page}`)
    }
  }
}

// ---------- 2) 锚点解析 ----------
function extractHeadings(filePath) {
  const src = readFileSync(filePath, 'utf8')
  // 跳过 frontmatter
  const m = src.match(/^---\n[\s\S]*?\n---\n/)
  const body = m ? src.slice(m[0].length) : src

  // 跳过 fenced code blocks（避免把代码里的 ## 当 heading）
  const stripped = body.replace(/^```[\s\S]*?^```/gm, '').replace(/^~~~[\s\S]*?^~~~/gm, '')

  const slugger = new GithubSlugger()
  const slugs = new Set()
  for (const line of stripped.split('\n')) {
    const h = line.match(/^(#{1,6})\s+(.+?)\s*$/)
    if (!h) continue
    const text = h[2]
      // 把 markdown 行内格式剥掉，保留可见文本
      .replace(/`([^`]+)`/g, '$1')
      .replace(/\*\*([^*]+)\*\*/g, '$1')
      .replace(/\*([^*]+)\*/g, '$1')
      .replace(/_([^_]+)_/g, '$1')
    slugs.add(slugger.slug(text, false))
  }
  return slugs
}

const headingCache = new Map()
function headingsFor(filePath) {
  if (!headingCache.has(filePath)) headingCache.set(filePath, extractHeadings(filePath))
  return headingCache.get(filePath)
}

// 提取 MDX 中所有 (./.. 或 ../) 开头的相对链接，可带 #anchor
const REL_LINK = /\]\(((?:\.{1,2})\/[^\s)]*?)(?:#([^\s)]+))?\)/g
// 站内绝对链接：(/zh/...) / (/docs/...) — 暂不处理（来自 fumadocs basePath，运行时再校验）

for (const lang of LANGS) {
  for (const rel of langPages[lang]) {
    const filePath = join(DOCS_ROOT, lang, rel)
    const src = readFileSync(filePath, 'utf8')

    // 3) bare autolink 检查（MDX 3 兼容性）
    if (/<https?:\/\//.test(src)) {
      errors.push(
        `${lang}/${rel}: 含裸 autolink <https://...>，MDX 3 会按 JSX 解析。改成 [text](url)`
      )
    }

    let match
    REL_LINK.lastIndex = 0
    while ((match = REL_LINK.exec(src))) {
      const [, target, anchor] = match
      // 解析目标到绝对路径
      const fileDir = dirname(filePath)
      let resolved = normalize(join(fileDir, target))
      // 末尾不带扩展名时尝试 .mdx
      if (!resolved.endsWith('.mdx')) {
        if (resolved.endsWith('/')) resolved = resolved.slice(0, -1)
        resolved = resolved + '.mdx'
      }
      // 必须落在同语言树内
      const expectedRoot = join(DOCS_ROOT, lang)
      if (!resolved.startsWith(expectedRoot)) {
        warnings.push(`${lang}/${rel}: 链接逃出同语言树：${target}`)
        continue
      }
      let ok = true
      try {
        statSync(resolved)
      } catch {
        errors.push(`${lang}/${rel}: 链接目标文件不存在 → ${target}`)
        ok = false
      }
      if (!ok) continue
      if (anchor) {
        const slugs = headingsFor(resolved)
        if (!slugs.has(anchor)) {
          errors.push(
            `${lang}/${rel}: 锚点 #${anchor} 在 ${relative(DOCS_ROOT, resolved)} 中不存在`
          )
        }
      }
    }
  }
}

// ---------- 报告 ----------
if (warnings.length) {
  console.warn('Warnings:')
  for (const w of warnings) console.warn('  ' + w)
}

if (errors.length) {
  console.error('Errors:')
  for (const e of errors) console.error('  ' + e)
  console.error(`\n${errors.length} error(s).`)
  process.exit(1)
}

console.log(
  `OK — ${langPages.en.length} en pages, ${langPages.zh.length} zh pages, anchors check passed.`
)
