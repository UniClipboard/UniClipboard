import { createMDX } from 'fumadocs-mdx/next'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const __dirname = dirname(fileURLToPath(import.meta.url))

const withMDX = createMDX()

// Old documentation URLs kept alive after the 2026 category restructure:
// guides/* split into core-features/* and help/*, and the CLI page was
// promoted to a top-level /cli group. `source` is matched after `basePath`
// (/docs) is stripped, so paths are written without the /docs prefix. Each
// move needs an en variant (default locale, no prefix) and a zh variant.
// Pages that did not move (guides/pairing, guides/settings,
// guides/self-host-relay, reference/mobile-api, reference/mobile-connect-uri,
// reference/search-internals, getting-started/*) are intentionally absent.
const docPageMoves = [
  ['/guides/sync', '/core-features/sync'],
  ['/guides/search', '/core-features/search'],
  ['/guides/devices', '/core-features/devices'],
  ['/guides/quick-panel', '/core-features/quick-panel'],
  ['/guides/mobile-sync', '/core-features/mobile-sync'],
  ['/guides/privacy', '/core-features/privacy'],
  ['/guides/troubleshooting', '/help/troubleshooting'],
  ['/reference/cli', '/cli/getting-started'],
  ['/cli', '/cli/getting-started'],
  ['/faq', '/help/faq'],
]

const docRedirects = docPageMoves.flatMap(([from, to]) => [
  { source: from, destination: to, permanent: true },
  { source: `/zh${from}`, destination: `/zh${to}`, permanent: true },
])

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  basePath: '/docs',
  assetPrefix: '/docs',
  async redirects() {
    return docRedirects
  },
  // Pin Turbopack and the file-tracing root to docs-site so the monorepo
  // root's lockfile / package.json doesn't hijack module resolution.
  turbopack: {
    root: __dirname,
  },
  // `outputFileTracingRoot` is a third belt over `pnpm-workspace.yaml`
  // (find-root marker) and `turbopack.root` above. On Vercel + Next.js 16
  // it triggers a build-output mismatch — `.next/routes-manifest-deterministic.json`
  // ends up missing and the deploy step fails with ENOENT. Vercel's Root
  // Directory setting (`docs-site`) already isolates the build, so the
  // pin is unnecessary there.
  ...(process.env.VERCEL ? {} : { outputFileTracingRoot: __dirname }),
}

export default withMDX(config)
