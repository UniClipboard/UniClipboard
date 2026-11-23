import { createMDX } from 'fumadocs-mdx/next'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const __dirname = dirname(fileURLToPath(import.meta.url))

const withMDX = createMDX()

// v2 文档站重写后不保留 v1 页面重定向。
const docPageMoves = []

const docRedirects = docPageMoves.flatMap(([from, to]) => [
  { source: from, destination: to, permanent: true },
  { source: `/zh${from}`, destination: `/zh${to}`, permanent: true },
])

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  async redirects() {
    return docRedirects
  },
  // Pin Turbopack and the file-tracing root to docs-site so the monorepo
  // root's lockfile / package.json doesn't hijack module resolution.
  turbopack: {
    root: __dirname,
  },
  // Keep Next.js file tracing scoped to docs-site.
  outputFileTracingRoot: __dirname,
}

export default withMDX(config)
